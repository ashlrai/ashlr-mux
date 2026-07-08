import { describe, expect, test } from "bun:test";

import {
  KEYBOARD_RESIZE_STEP_PX,
  MAX_DIVIDER,
  MIN_DIVIDER,
  clampDivider,
  firstActivePanelId,
  countLeaves,
  equalizeDivider,
  keyboardDividerResize,
  resizeDivider,
  setDividerAtPath,
  type Layout,
  type Split,
} from "./splitLayout";

function pane(...panelIds: string[]): Layout {
  return { type: "pane", pane: { panel_ids: panelIds } };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

describe("clampDivider", () => {
  test("passes values already in range through", () => {
    expect(clampDivider(0.5)).toBe(0.5);
    expect(clampDivider(MIN_DIVIDER)).toBe(MIN_DIVIDER);
    expect(clampDivider(MAX_DIVIDER)).toBe(MAX_DIVIDER);
  });

  test("clamps out-of-range values to [0.1, 0.9]", () => {
    expect(clampDivider(-1)).toBe(MIN_DIVIDER);
    expect(clampDivider(0)).toBe(MIN_DIVIDER);
    expect(clampDivider(1)).toBe(MAX_DIVIDER);
    expect(clampDivider(42)).toBe(MAX_DIVIDER);
  });

  test("NaN falls back to centered", () => {
    expect(clampDivider(Number.NaN)).toBe(0.5);
  });
});

describe("countLeaves", () => {
  test("a bare pane is one leaf", () => {
    expect(countLeaves(pane("a"))).toBe(1);
  });

  test("counts leaves across a nested tree", () => {
    // split( pane, split( pane, pane ) ) => 3 leaves
    const tree = split("horizontal", 0.5, pane("a"), split("vertical", 0.5, pane("b"), pane("c")));
    expect(countLeaves(tree)).toBe(3);
  });
});

describe("resizeDivider", () => {
  test("adds delta/axis to the current ratio", () => {
    // 0.5 + 100/400 = 0.75
    expect(resizeDivider(0.5, 100, 400)).toBeCloseTo(0.75, 10);
  });

  test("negative delta drags the divider back", () => {
    expect(resizeDivider(0.5, -100, 400)).toBeCloseTo(0.25, 10);
  });

  test("clamps the result into the legal range", () => {
    expect(resizeDivider(0.8, 1000, 400)).toBe(MAX_DIVIDER);
    expect(resizeDivider(0.2, -1000, 400)).toBe(MIN_DIVIDER);
  });

  test("a non-positive axis is a no-op (returns the clamped current)", () => {
    expect(resizeDivider(0.5, 100, 0)).toBe(0.5);
    expect(resizeDivider(0.05, 100, -10)).toBe(MIN_DIVIDER);
  });
});

describe("keyboardDividerResize", () => {
  test("one step is Ghostty's default resize_split amount (10px)", () => {
    expect(KEYBOARD_RESIZE_STEP_PX).toBe(10);
  });

  test("horizontal split: ArrowRight/ArrowLeft step the ratio by step/axis", () => {
    // 0.5 + 10/400 = 0.525; 0.5 - 10/400 = 0.475.
    expect(keyboardDividerResize("horizontal", 0.5, "ArrowRight", 400)).toBeCloseTo(0.525, 10);
    expect(keyboardDividerResize("horizontal", 0.5, "ArrowLeft", 400)).toBeCloseTo(0.475, 10);
  });

  test("vertical split: ArrowDown/ArrowUp step the ratio by step/axis", () => {
    expect(keyboardDividerResize("vertical", 0.5, "ArrowDown", 400)).toBeCloseTo(0.525, 10);
    expect(keyboardDividerResize("vertical", 0.5, "ArrowUp", 400)).toBeCloseTo(0.475, 10);
  });

  test("cross-axis arrows are null (parity with macOS orientation-mismatch nil)", () => {
    expect(keyboardDividerResize("horizontal", 0.5, "ArrowUp", 400)).toBeNull();
    expect(keyboardDividerResize("horizontal", 0.5, "ArrowDown", 400)).toBeNull();
    expect(keyboardDividerResize("vertical", 0.5, "ArrowLeft", 400)).toBeNull();
    expect(keyboardDividerResize("vertical", 0.5, "ArrowRight", 400)).toBeNull();
  });

  test("non-arrow keys are null (no Home/End equivalent in the oracle)", () => {
    expect(keyboardDividerResize("horizontal", 0.5, "Home", 400)).toBeNull();
    expect(keyboardDividerResize("horizontal", 0.5, "End", 400)).toBeNull();
    expect(keyboardDividerResize("vertical", 0.5, "a", 400)).toBeNull();
  });

  test("clamps the stepped ratio into [0.1, 0.9]", () => {
    // Step = 10/100 = 0.1; 0.89 + 0.1 would be 0.99 -> MAX; 0.11 - 0.1 -> MIN.
    expect(keyboardDividerResize("horizontal", 0.89, "ArrowRight", 100)).toBe(MAX_DIVIDER);
    expect(keyboardDividerResize("horizontal", 0.11, "ArrowLeft", 100)).toBe(MIN_DIVIDER);
  });

  test("a degenerate axis returns the clamped current ratio", () => {
    expect(keyboardDividerResize("horizontal", 0.5, "ArrowRight", 0)).toBe(0.5);
    expect(keyboardDividerResize("vertical", 0.95, "ArrowUp", 0)).toBe(MAX_DIVIDER);
  });
});

describe("equalizeDivider", () => {
  test("two equal leaves centre the divider", () => {
    const s = { orientation: "horizontal", divider_position: 0.8, first: pane("a"), second: pane("b") } as Split;
    expect(equalizeDivider(s)).toBe(0.5);
  });

  // DELIBERATE CORRECTION: previously asserted 2/3 (leaf-weighted), which was
  // WRONG vs macOS. A cross-orientation nested subtree counts as ONE span, so a
  // horizontal split over a vertical [a,b] and a leaf c equalizes to 1/2.
  test("weights by orientation-aware span; a cross-orientation subtree is 1 span (=> 1/2)", () => {
    const s = {
      orientation: "horizontal",
      divider_position: 0.5,
      first: split("vertical", 0.5, pane("a"), pane("b")),
      second: pane("c"),
    } as Split;
    expect(equalizeDivider(s)).toBeCloseTo(1 / 2, 10);
  });

  test("weights same-orientation spans by their span count (2 vs 1 => 2/3)", () => {
    const s = {
      orientation: "horizontal",
      divider_position: 0.5,
      first: split("horizontal", 0.5, pane("a"), pane("b")),
      second: pane("c"),
    } as Split;
    expect(equalizeDivider(s)).toBeCloseTo(2 / 3, 10);
  });

  test("an extreme span ratio is NOT clamped (parity with macOS equalize)", () => {
    // 10 same-orientation spans vs 1 => 10/11 (~0.909), ABOVE MAX_DIVIDER.
    // macOS `appendEqualizeAdjustments` emits the raw span ratio with no clamp,
    // so equalize must return 10/11, not MAX_DIVIDER. The old 9-vs-1 case landed
    // exactly on 0.9 == MAX_DIVIDER and so passed with or without a clamp.
    let left: Layout = pane("l0");
    for (let i = 1; i < 10; i += 1) {
      left = split("horizontal", 0.5, left, pane(`l${i}`));
    }
    const s = { orientation: "horizontal", divider_position: 0.5, first: left, second: pane("r") } as Split;
    expect(countLeaves(left)).toBe(10);
    expect(equalizeDivider(s)).toBeCloseTo(10 / 11, 10);
    // Sanity: 10/11 is strictly greater than the clamp ceiling, so a stray
    // clampDivider would visibly break this assertion.
    expect(equalizeDivider(s)).toBeGreaterThan(MAX_DIVIDER);
  });
});

describe("setDividerAtPath", () => {
  const tree = split(
    "horizontal",
    0.5,
    pane("a"),
    split("vertical", 0.5, pane("b"), pane("c")),
  );

  test("empty path updates the root split", () => {
    const next = setDividerAtPath(tree, [], 0.3);
    expect(next.type).toBe("split");
    if (next.type === "split") {
      expect(next.split.divider_position).toBe(0.3);
    }
  });

  test("updates a nested split addressed by path", () => {
    const next = setDividerAtPath(tree, ["second"], 0.2);
    if (next.type === "split" && next.split.second.type === "split") {
      expect(next.split.second.split.divider_position).toBe(0.2);
      // Root divider untouched.
      expect(next.split.divider_position).toBe(0.5);
    } else {
      throw new Error("expected a nested split");
    }
  });

  test("clamps the new position", () => {
    const next = setDividerAtPath(tree, [], 5);
    if (next.type === "split") {
      expect(next.split.divider_position).toBe(MAX_DIVIDER);
    }
  });

  test("a path running off a leaf is a no-op returning the same subtree ref", () => {
    // ["first"] is a pane; ["first","second"] runs past it.
    const next = setDividerAtPath(tree, ["first", "second"], 0.3);
    expect(next).toEqual(tree);
  });

  test("does not mutate the input tree", () => {
    const before = JSON.stringify(tree);
    setDividerAtPath(tree, ["second"], 0.1);
    expect(JSON.stringify(tree)).toBe(before);
  });
});

describe("firstActivePanelId", () => {
  test("a pane reports its selected panel, else its first", () => {
    expect(firstActivePanelId(pane("a", "b"))).toBe("a");
    const selected: Layout = {
      type: "pane",
      pane: { panel_ids: ["a", "b"], selected_panel_id: "b" },
    };
    expect(firstActivePanelId(selected)).toBe("b");
  });

  test("a split reports the first (top-left-most) leaf's panel", () => {
    const layout = split(
      "horizontal",
      0.5,
      split("vertical", 0.5, pane("tl"), pane("bl")),
      pane("r"),
    );
    expect(firstActivePanelId(layout)).toBe("tl");
  });

  test("an empty malformed pane yields undefined and a sibling fallback", () => {
    expect(firstActivePanelId(pane())).toBeUndefined();
    expect(firstActivePanelId(split("horizontal", 0.5, pane(), pane("r")))).toBe("r");
  });
});
