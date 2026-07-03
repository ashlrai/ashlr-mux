import { describe, expect, test } from "bun:test";

import type { Layout } from "./splitLayout";
import {
  RESIZE_DIRECTION,
  SPLIT_DIRECTION,
  equalizeDividerPlan,
  resizeDividerAdjustment,
} from "./splitGeometry";

function pane(id: string): Layout {
  return { type: "pane", pane: { panel_ids: [id] } };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

// Ports CmuxPanes/Tests/CmuxPanesTests/SplitGeometryTests.swift 1:1. Splits are
// addressed by PATH, not the Swift `splitId: UUID` — see the DIVERGENCE header
// in splitGeometry.ts. The Swift `equalizeFlagsUnparseableSplitIds` test is
// intentionally NOT ported: web paths are always valid, so there is no
// `hadInvalidSplitIds` state to assert.

describe("equalizeDividerPlan", () => {
  test("weights nested same-orientation spans", () => {
    // split( a, split( b, c ) ), both horizontal.
    const tree = split(
      "horizontal",
      0.8,
      pane("a"),
      split("horizontal", 0.2, pane("b"), pane("c")),
    );

    const plan = equalizeDividerPlan(tree);

    expect(plan.foundSplit).toBe(true);
    // Children are planned before their parent (legacy post-order): inner
    // (["second"]) then outer ([]).
    expect(plan.adjustments.map((a) => a.path)).toEqual([["second"], []]);
    // Inner split divides its two leaves evenly; the outer divider gives its
    // first child 1 of 3 same-orientation spans.
    expect(plan.adjustments[0].position).toBe(0.5);
    expect(plan.adjustments[1].position).toBeCloseTo(1 / 3, 10);
  });

  test("treats a cross-orientation subtree as one span", () => {
    const tree = split(
      "horizontal",
      0.5,
      pane("a"),
      split("vertical", 0.5, pane("b"), pane("c")),
    );

    const plan = equalizeDividerPlan(tree);

    // The vertical subtree counts as a single horizontal span, so the outer
    // divider lands at 1/2.
    expect(plan.adjustments.find((a) => a.path.length === 0)?.position).toBe(0.5);
    expect(plan.adjustments.find((a) => a.path.length === 1)?.position).toBe(0.5);
  });

  test("orientation filter skips other orientations", () => {
    const tree = split(
      "horizontal",
      0.5,
      pane("a"),
      split("vertical", 0.5, pane("b"), pane("c")),
    );

    const plan = equalizeDividerPlan(tree, "vertical");

    expect(plan.foundSplit).toBe(true);
    expect(plan.adjustments.map((a) => a.path)).toEqual([["second"]]);

    const noMatch = equalizeDividerPlan(pane("solo"), "vertical");
    expect(noMatch.foundSplit).toBe(false);
    expect(noMatch.adjustments).toEqual([]);
  });
});

describe("resizeDividerAdjustment", () => {
  // Container pixels stand in for the Swift pane frames: the split's axis
  // length in px = container.(w|h) * (its percentage span / 100), mirroring
  // Workspace.tsx beginDividerDrag.

  test("moves the controlling divider by the pixel delta", () => {
    const tree = split(
      "horizontal",
      0.5,
      pane("a"),
      pane("b"),
    );

    // Target the second child; .left controls a divider whose target sits in
    // the second child and moves it toward the first child.
    const adjustment = resizeDividerAdjustment(tree, "b", "left", 60, { width: 600, height: 400 });

    expect(adjustment?.path).toEqual([]);
    // 60px over a 600px axis = 0.1 delta, signed negative for .left.
    expect(adjustment).not.toBeNull();
    expect(Math.abs((adjustment as { position: number }).position - 0.4)).toBeLessThan(0.0001);
  });

  test("requires a matching child side", () => {
    const tree = split("horizontal", 0.5, pane("a"), pane("b"));
    const container = { width: 600, height: 400 };

    // .right requires the target in the first child; "b" is the second.
    expect(resizeDividerAdjustment(tree, "b", "right", 10, container)).toBeNull();
    // Vertical resize has no vertical split to control.
    expect(resizeDividerAdjustment(tree, "b", "up", 10, container)).toBeNull();
    // Unknown pane plans nothing.
    expect(resizeDividerAdjustment(tree, "zz", "left", 10, container)).toBeNull();
  });

  test("prefers the innermost enclosing split", () => {
    const tree = split(
      "horizontal",
      0.5,
      pane("a"),
      split("horizontal", 0.5, pane("b"), pane("c")),
    );

    // "c" sits in the second child of BOTH splits; the innermost (closest
    // enclosing) candidate wins, matching the legacy order.
    const adjustment = resizeDividerAdjustment(tree, "c", "left", 30, { width: 600, height: 400 });
    expect(adjustment?.path).toEqual(["second"]);
  });

  test("clamps the divider to the legacy bounds", () => {
    const tree = split("vertical", 0.85, pane("a"), pane("b"));

    // A huge downward move from 0.85 clamps to 0.9.
    const adjustment = resizeDividerAdjustment(tree, "a", "down", 400, { width: 600, height: 400 });
    expect(adjustment?.position).toBe(0.9);
  });
});

describe("direction value maps", () => {
  test("SplitDirection maps orientation and insertion side", () => {
    expect(SPLIT_DIRECTION.left.isHorizontal).toBe(true);
    expect(SPLIT_DIRECTION.right.isHorizontal).toBe(true);
    expect(SPLIT_DIRECTION.up.isHorizontal).toBe(false);
    expect(SPLIT_DIRECTION.left.orientation).toBe("horizontal");
    expect(SPLIT_DIRECTION.down.orientation).toBe("vertical");
    expect(SPLIT_DIRECTION.left.insertFirst).toBe(true);
    expect(SPLIT_DIRECTION.up.insertFirst).toBe(true);
    expect(SPLIT_DIRECTION.right.insertFirst).toBe(false);
    expect(SPLIT_DIRECTION.down.insertFirst).toBe(false);
  });

  test("ResizeDirection maps split axis and sign", () => {
    expect(RESIZE_DIRECTION.left.splitOrientation).toBe("horizontal");
    expect(RESIZE_DIRECTION.down.splitOrientation).toBe("vertical");
    expect(RESIZE_DIRECTION.right.requiresPaneInFirstChild).toBe(true);
    expect(RESIZE_DIRECTION.down.requiresPaneInFirstChild).toBe(true);
    expect(RESIZE_DIRECTION.left.requiresPaneInFirstChild).toBe(false);
    expect(RESIZE_DIRECTION.right.dividerDeltaSign).toBe(1);
    expect(RESIZE_DIRECTION.up.dividerDeltaSign).toBe(-1);
  });
});
