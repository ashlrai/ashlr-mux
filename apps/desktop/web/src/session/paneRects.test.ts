import { describe, expect, test } from "bun:test";

import { dividerHandles, paneRects, surfaceKinds, type Rect } from "./paneRects";
import type { Layout } from "./splitLayout";

function pane(...panelIds: string[]): Layout {
  return { type: "pane", pane: { panel_ids: panelIds } };
}

/** A pane with an explicit `surface_kind` (raw, pre-normalization). */
function paneKind(surfaceKind: string | undefined, ...panelIds: string[]): Layout {
  return { type: "pane", pane: { panel_ids: panelIds, surface_kind: surfaceKind } };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

function approx(rect: Rect | undefined, expected: Rect): void {
  expect(rect).toBeDefined();
  expect(rect!.x).toBeCloseTo(expected.x, 6);
  expect(rect!.y).toBeCloseTo(expected.y, 6);
  expect(rect!.w).toBeCloseTo(expected.w, 6);
  expect(rect!.h).toBeCloseTo(expected.h, 6);
}

describe("paneRects", () => {
  test("a single pane fills the whole container", () => {
    const rects = paneRects(pane("surface-1"));
    expect(rects.size).toBe(1);
    approx(rects.get("surface-1"), { x: 0, y: 0, w: 100, h: 100 });
  });

  test("a horizontal split places children side by side at the ratio", () => {
    const rects = paneRects(split("horizontal", 0.4, pane("a"), pane("b")));
    approx(rects.get("a"), { x: 0, y: 0, w: 40, h: 100 });
    approx(rects.get("b"), { x: 40, y: 0, w: 60, h: 100 });
  });

  test("a vertical split stacks children at the ratio", () => {
    const rects = paneRects(split("vertical", 0.25, pane("top"), pane("bottom")));
    approx(rects.get("top"), { x: 0, y: 0, w: 100, h: 25 });
    approx(rects.get("bottom"), { x: 0, y: 25, w: 100, h: 75 });
  });

  test("nested splits compose their rects relative to the parent", () => {
    // Left half is a pane; right half is split vertically 50/50.
    const layout = split(
      "horizontal",
      0.5,
      pane("left"),
      split("vertical", 0.5, pane("tr"), pane("br")),
    );
    const rects = paneRects(layout);
    approx(rects.get("left"), { x: 0, y: 0, w: 50, h: 100 });
    approx(rects.get("tr"), { x: 50, y: 0, w: 50, h: 50 });
    approx(rects.get("br"), { x: 50, y: 50, w: 50, h: 50 });
  });

  test("out-of-range divider ratios are clamped like the renderer", () => {
    const rects = paneRects(split("horizontal", 5, pane("a"), pane("b")));
    // clampDivider caps at 0.9.
    approx(rects.get("a"), { x: 0, y: 0, w: 90, h: 100 });
    approx(rects.get("b"), { x: 90, y: 0, w: 10, h: 100 });
  });

  test("keys by the selected panel id when present", () => {
    const layout: Layout = {
      type: "pane",
      pane: { panel_ids: ["surface-1", "surface-9"], selected_panel_id: "surface-9" },
    };
    const rects = paneRects(layout);
    expect([...rects.keys()]).toEqual(["surface-9"]);
  });
});

describe("dividerHandles", () => {
  test("a single pane has no dividers", () => {
    expect(dividerHandles(pane("solo"))).toEqual([]);
  });

  test("emits one handle per split with its path, orientation and ratio", () => {
    const layout = split(
      "horizontal",
      0.5,
      pane("left"),
      split("vertical", 0.6, pane("tr"), pane("br")),
    );
    const handles = dividerHandles(layout);
    expect(handles.length).toBe(2);

    const root = handles[0];
    expect(root.path).toEqual([]);
    expect(root.orientation).toBe("horizontal");
    expect(root.ratio).toBeCloseTo(0.5, 6);
    approx(root.parent, { x: 0, y: 0, w: 100, h: 100 });

    const nested = handles[1];
    expect(nested.path).toEqual(["second"]);
    expect(nested.orientation).toBe("vertical");
    expect(nested.ratio).toBeCloseTo(0.6, 6);
    // The nested split lives in the right half.
    approx(nested.parent, { x: 50, y: 0, w: 50, h: 100 });
  });
});

describe("surfaceKinds", () => {
  test("maps each recognized surface_kind through normalizeSurfaceKind", () => {
    const kinds = surfaceKinds(paneKind("agent", "a"));
    expect(kinds.get("a")).toBe("agent");
    expect(surfaceKinds(paneKind("markdown", "m")).get("m")).toBe("markdown");
    expect(surfaceKinds(paneKind("diff", "d")).get("d")).toBe("diff");
  });

  test("an explicit terminal / absent / unknown kind resolves to terminal", () => {
    expect(surfaceKinds(paneKind("terminal", "t")).get("t")).toBe("terminal");
    // Absent surface_kind (a plain pane) → terminal.
    expect(surfaceKinds(pane("bare")).get("bare")).toBe("terminal");
    // Unknown/future tag → terminal (not passed through).
    expect(surfaceKinds(paneKind("browser", "u")).get("u")).toBe("terminal");
  });

  test("keys by the selected panel id, like paneRects", () => {
    const layout: Layout = {
      type: "pane",
      pane: { panel_ids: ["surface-1", "surface-9"], selected_panel_id: "surface-9", surface_kind: "markdown" },
    };
    const kinds = surfaceKinds(layout);
    expect([...kinds.keys()]).toEqual(["surface-9"]);
    expect(kinds.get("surface-9")).toBe("markdown");
  });

  test("walks a nested tree and routes each leaf's kind independently", () => {
    const layout = split(
      "horizontal",
      0.5,
      paneKind("agent", "left"),
      split("vertical", 0.5, paneKind("diff", "tr"), pane("br")),
    );
    const kinds = surfaceKinds(layout);
    expect(kinds.get("left")).toBe("agent");
    expect(kinds.get("tr")).toBe("diff");
    expect(kinds.get("br")).toBe("terminal");
    expect(kinds.size).toBe(3);
  });

  test("skips a pane with no panel ids (nothing to key)", () => {
    const empty: Layout = { type: "pane", pane: { panel_ids: [] } };
    const layout = split("horizontal", 0.5, empty, paneKind("markdown", "only"));
    const kinds = surfaceKinds(layout);
    expect([...kinds.keys()]).toEqual(["only"]);
  });
});
