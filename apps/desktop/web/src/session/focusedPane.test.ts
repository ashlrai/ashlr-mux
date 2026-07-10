import { describe, expect, test } from "bun:test";

import {
  adjacentCanvasPanelId,
  adjacentPanelId,
  createFocusedPaneStore,
  paneIdForPanelId,
  panelIdsInLayout,
  resolveActivePanelId,
  resolveActivePaneId,
} from "./focusedPane";
import type { SessionCanvasPaneSnapshot } from "@cmux/core-types";
import type { Layout } from "./splitLayout";

function pane(panelIds: string[], selected?: string, paneId?: string): Layout {
  return {
    type: "pane",
    pane: {
      panel_ids: panelIds,
      selected_panel_id: selected,
      pane_id: paneId,
    },
  };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

describe("createFocusedPaneStore", () => {
  test("starts with no focused pane", () => {
    expect(createFocusedPaneStore().get()).toBeUndefined();
  });

  test("focus records the panel id and get returns it", () => {
    const store = createFocusedPaneStore();
    store.focus("a");
    expect(store.get()).toBe("a");
    store.focus("b");
    expect(store.get()).toBe("b");
  });

  test("subscribers are notified on change", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    expect(notified).toBe(1);
    store.focus("b");
    expect(notified).toBe(2);
  });

  test("re-focusing the same id does NOT notify (event-spam no-op)", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    store.focus("a");
    store.focus("a");
    expect(notified).toBe(1);
    expect(store.get()).toBe("a");
  });

  test("unsubscribe stops notifications", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    const unsubscribe = store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    unsubscribe();
    store.focus("b");
    expect(notified).toBe(1);
    expect(store.get()).toBe("b");
  });

  test("clear resets to no focus and notifies; clearing empty is a no-op", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.clear();
    expect(notified).toBe(0);
    store.focus("a");
    store.clear();
    expect(store.get()).toBeUndefined();
    expect(notified).toBe(2);
  });
});

describe("panelIdsInLayout", () => {
  test("collects every panel id of every leaf, not just the selected ones", () => {
    const layout = split(
      "horizontal",
      0.5,
      pane(["a1", "a2"], "a1"),
      split("vertical", 0.5, pane(["b"]), pane(["c"])),
    );
    expect([...panelIdsInLayout(layout)].sort()).toEqual(["a1", "a2", "b", "c"]);
  });
});

describe("paneIdForPanelId", () => {
  test("returns the owning pane id for a selected or non-selected panel", () => {
    const layout = split(
      "horizontal",
      0.5,
      pane(["a1", "a2"], "a1", "pane-left"),
      pane(["b"], "b", "pane-right"),
    );
    expect(paneIdForPanelId(layout, "a1")).toBe("pane-left");
    expect(paneIdForPanelId(layout, "a2")).toBe("pane-left");
    expect(paneIdForPanelId(layout, "b")).toBe("pane-right");
  });

  test("returns undefined when the layout carries no pane id or panel match", () => {
    expect(paneIdForPanelId(pane(["a"]), "missing")).toBeUndefined();
    expect(paneIdForPanelId(pane(["a"]), "a")).toBeUndefined();
  });
});

describe("resolveActivePanelId", () => {
  const twoPanes = split("horizontal", 0.5, pane(["a"]), pane(["b"]));

  test("no layout resolves to undefined regardless of focus", () => {
    expect(resolveActivePanelId("a", undefined)).toBeUndefined();
    expect(resolveActivePanelId("a", null)).toBeUndefined();
    expect(resolveActivePanelId(undefined, undefined)).toBeUndefined();
  });

  test("a focused id living in the first leaf is returned verbatim", () => {
    expect(resolveActivePanelId("a", twoPanes)).toBe("a");
  });

  test("a focused id living in a NON-first leaf is returned verbatim", () => {
    expect(resolveActivePanelId("b", twoPanes)).toBe("b");
  });

  test("a focused id that is a non-selected member of a pane's panel_ids still resolves", () => {
    const layout = split("horizontal", 0.5, pane(["a1", "a2"], "a1"), pane(["b"]));
    expect(resolveActivePanelId("a2", layout)).toBe("a2");
  });

  test("a stale focused id falls back to the first active panel", () => {
    expect(resolveActivePanelId("gone", twoPanes)).toBe("a");
  });

  test("no focus falls back to the first active panel", () => {
    expect(resolveActivePanelId(undefined, twoPanes)).toBe("a");
  });

  test("a malformed empty pane resolves to undefined", () => {
    expect(resolveActivePanelId(undefined, pane([]))).toBeUndefined();
  });

  test("workspace switch: the raw focus survives and re-resolves per layout", () => {
    // Focus "b" in workspace 1, switch to workspace 2 (no "b") — falls back to
    // workspace 2's first leaf; switch back — the remembered focus resolves
    // again. Panel ids are globally unique, so the stale id cannot collide.
    const store = createFocusedPaneStore();
    const workspace2 = split("vertical", 0.5, pane(["x"]), pane(["y"]));
    store.focus("b");
    expect(resolveActivePanelId(store.get(), workspace2)).toBe("x");
    expect(resolveActivePanelId(store.get(), twoPanes)).toBe("b");
  });
});

describe("resolveActivePaneId", () => {
  const twoPanes = split(
    "horizontal",
    0.5,
    pane(["a"], "a", "pane-a"),
    pane(["b"], "b", "pane-b"),
  );

  test("no layout resolves to undefined regardless of focus", () => {
    expect(resolveActivePaneId("a", undefined)).toBeUndefined();
    expect(resolveActivePaneId("a", null)).toBeUndefined();
  });

  test("focused and fallback resolution returns the owning pane id", () => {
    expect(resolveActivePaneId("b", twoPanes)).toBe("pane-b");
    expect(resolveActivePaneId(undefined, twoPanes)).toBe("pane-a");
  });

  test("returns undefined when the resolved pane lacks a pane id", () => {
    expect(resolveActivePaneId("a", pane(["a"]))).toBeUndefined();
  });
});

describe("adjacentPanelId", () => {
  const quadrant = split(
    "horizontal",
    0.5,
    split("vertical", 0.5, pane(["top-left"]), pane(["bottom-left"])),
    split("vertical", 0.5, pane(["top-right"]), pane(["bottom-right"])),
  );

  test("moves left, right, up, and down by flat-portal pane geometry", () => {
    expect(adjacentPanelId(quadrant, "top-left", "right")).toBe("top-right");
    expect(adjacentPanelId(quadrant, "top-right", "left")).toBe("top-left");
    expect(adjacentPanelId(quadrant, "top-left", "down")).toBe("bottom-left");
    expect(adjacentPanelId(quadrant, "bottom-left", "up")).toBe("top-left");
  });

  test("prefers an overlapping neighbor over a diagonal-only candidate", () => {
    const layout = split(
      "horizontal",
      0.5,
      split("vertical", 0.5, pane(["upper-left"]), pane(["lower-left"])),
      pane(["right-tall"]),
    );
    expect(adjacentPanelId(layout, "upper-left", "right")).toBe("right-tall");
    expect(adjacentPanelId(layout, "lower-left", "right")).toBe("right-tall");
  });

  test("returns undefined at an edge or without a live current pane", () => {
    expect(adjacentPanelId(quadrant, "top-left", "left")).toBeUndefined();
    expect(adjacentPanelId(quadrant, "missing", "right")).toBeUndefined();
    expect(adjacentPanelId(null, "top-left", "right")).toBeUndefined();
  });
});

describe("adjacentCanvasPanelId", () => {
  const panes: SessionCanvasPaneSnapshot[] = [
    { panel_id: "left", x: 0, y: 0, width: 200, height: 120 },
    { panel_id: "right", x: 260, y: 0, width: 200, height: 120 },
    { panel_id: "down", x: 0, y: 180, width: 200, height: 120 },
    { panel_id: "diagonal", x: 260, y: 220, width: 200, height: 120 },
  ];

  test("moves by persisted canvas pane geometry", () => {
    expect(adjacentCanvasPanelId(panes, "left", "right")).toBe("right");
    expect(adjacentCanvasPanelId(panes, "right", "left")).toBe("left");
    expect(adjacentCanvasPanelId(panes, "left", "down")).toBe("down");
    expect(adjacentCanvasPanelId(panes, "down", "up")).toBe("left");
  });

  test("prefers overlapping canvas neighbors over diagonal candidates", () => {
    expect(adjacentCanvasPanelId(panes, "down", "right")).toBe("diagonal");
    expect(adjacentCanvasPanelId(panes, "left", "right")).toBe("right");
  });

  test("matches non-selected tab ids and returns the selected panel key", () => {
    const tabbed: SessionCanvasPaneSnapshot[] = [
      {
        panel_id: "pane-a",
        selected_panel_id: "a1",
        panel_ids: ["a1", "a2"],
        x: 0,
        y: 0,
        width: 100,
        height: 100,
      },
      { panel_id: "b", x: 160, y: 0, width: 100, height: 100 },
    ];
    expect(adjacentCanvasPanelId(tabbed, "a2", "right")).toBe("b");
    expect(adjacentCanvasPanelId(tabbed, "b", "left")).toBe("a1");
  });

  test("returns undefined at an edge or without a live current pane", () => {
    expect(adjacentCanvasPanelId(panes, "left", "left")).toBeUndefined();
    expect(adjacentCanvasPanelId(panes, "missing", "right")).toBeUndefined();
    expect(adjacentCanvasPanelId(undefined, "left", "right")).toBeUndefined();
  });
});
