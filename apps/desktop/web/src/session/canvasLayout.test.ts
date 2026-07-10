import { describe, expect, test } from "bun:test";

import { canvasPoint, canvasRect } from "./canvasGeometry";
import {
  CanvasLayout,
  createCanvasPane,
  createCanvasPaneWithPanels,
} from "./canvasLayout";

function singleTabPane(id: string, x: number) {
  return createCanvasPane(id, canvasRect(x, 0, 300, 200));
}

describe("CanvasLayout geometry and z-order", () => {
  test("adds, removes, and looks up panes", () => {
    const layout = new CanvasLayout();
    expect(layout.isEmpty()).toBe(true);
    layout.add(createCanvasPane("a", canvasRect(0, 0, 100, 100)));
    expect(layout.contains("a")).toBe(true);
    expect(layout.frame("a")).toEqual(canvasRect(0, 0, 100, 100));
    layout.remove("a");
    expect(layout.contains("a")).toBe(false);
    expect(layout.frame("a")).toBeUndefined();
  });

  test("adding an existing pane replaces and raises it", () => {
    const layout = new CanvasLayout();
    layout.add(createCanvasPane("a", canvasRect(0, 0, 10, 10)));
    layout.add(createCanvasPane("b", canvasRect(20, 0, 10, 10)));
    layout.add(createCanvasPane("a", canvasRect(40, 0, 10, 10)));
    expect(layout.panes()).toHaveLength(2);
    expect(layout.paneIds()).toEqual(["b", "a"]);
    expect(layout.frame("a")).toEqual(canvasRect(40, 0, 10, 10));
  });

  test("bringToFront preserves front pane order", () => {
    const layout = new CanvasLayout([
      singleTabPane("a", 0),
      singleTabPane("b", 10),
      singleTabPane("c", 20),
    ]);
    layout.bringToFront("a");
    expect(layout.paneIds()).toEqual(["b", "c", "a"]);
    layout.bringToFront("a");
    expect(layout.paneIds()).toEqual(["b", "c", "a"]);
  });

  test("topPane hit-tests front-first", () => {
    const layout = new CanvasLayout([
      createCanvasPane("back", canvasRect(0, 0, 100, 100)),
      createCanvasPane("front", canvasRect(50, 50, 100, 100)),
    ]);
    expect(layout.topPane(canvasPoint(75, 75))).toBe("front");
    expect(layout.topPane(canvasPoint(10, 10))).toBe("back");
    expect(layout.topPane(canvasPoint(500, 500))).toBeUndefined();
  });

  test("contentBounds unions every pane", () => {
    const layout = new CanvasLayout();
    expect(layout.contentBounds()).toBeUndefined();
    layout.add(createCanvasPane("a", canvasRect(-10, 0, 20, 20)));
    layout.add(createCanvasPane("b", canvasRect(100, -50, 30, 30)));
    expect(layout.contentBounds()).toEqual(canvasRect(-10, -50, 140, 70));
  });

  test("setFrames applies a batch and ignores unknown ids", () => {
    const layout = new CanvasLayout([createCanvasPane("a", canvasRect(0, 0, 10, 10))]);
    layout.setFrames(new Map([
      ["a", canvasRect(5, 5, 10, 10)],
      ["unknown", canvasRect(99, 99, 1, 1)],
    ]));
    expect(layout.frame("a")).toEqual(canvasRect(5, 5, 10, 10));
    expect(layout.panes()).toHaveLength(1);
  });
});

describe("CanvasLayout panel hosting", () => {
  test("single-tab panes host their founding panel", () => {
    const pane = singleTabPane("pane-a", 0);
    expect(pane.panelIds).toEqual(["pane-a"]);
    expect(pane.selectedPanelId).toBe("pane-a");
  });

  test("addPanel joins, removes the old pane, and selects", () => {
    const layout = new CanvasLayout([singleTabPane("left", 0), singleTabPane("right", 400)]);
    layout.add(createCanvasPane("joining", canvasRect(800, 0, 300, 200)));
    layout.addPanel("joining", "left", undefined, true);
    expect(layout.panes()).toHaveLength(2);
    expect(layout.paneContaining("joining")).toBe("left");
    expect(layout.selectedPanelIdIn("left")).toBe("joining");
    expect(layout.panelIdsIn("left")).toHaveLength(2);
  });

  test("addPanel clamps index and can preserve selection", () => {
    const layout = new CanvasLayout([singleTabPane("left", 0)]);
    layout.add(createCanvasPane("a", canvasRect(400, 0, 300, 200)));
    layout.add(createCanvasPane("b", canvasRect(800, 0, 300, 200)));
    layout.addPanel("a", "left", 0, false);
    layout.addPanel("b", "left", 99, false);
    expect(layout.panelIdsIn("left")).toEqual(["a", "left", "b"]);
    expect(layout.selectedPanelIdIn("left")).toBe("left");
  });

  test("removePanel moves selection to nearest neighbor", () => {
    const layout = new CanvasLayout([singleTabPane("left", 0)]);
    layout.add(createCanvasPane("a", canvasRect(400, 0, 300, 200)));
    layout.addPanel("a", "left", undefined, true);
    expect(layout.removePanel("a")).toBe("left");
    expect(layout.panes()).toHaveLength(1);
    expect(layout.selectedPanelIdIn("left")).toBe("left");
  });

  test("removing the last panel removes its pane", () => {
    const layout = new CanvasLayout([singleTabPane("left", 0)]);
    layout.removePanel("left");
    expect(layout.isEmpty()).toBe(true);
  });

  test("breakOutPanel creates a frontmost single-tab pane", () => {
    const layout = new CanvasLayout([singleTabPane("left", 0), singleTabPane("right", 400)]);
    layout.add(createCanvasPane("joining", canvasRect(800, 0, 300, 200)));
    layout.addPanel("joining", "left", undefined, true);
    const frame = canvasRect(1200, 0, 300, 200);
    expect(layout.breakOutPanel("joining", "new-pane", frame)).toBe(true);
    expect(layout.paneContaining("joining")).toBe("new-pane");
    expect(layout.paneIds().at(-1)).toBe("new-pane");
    expect(layout.frame("new-pane")).toEqual(frame);
    expect(layout.breakOutPanel("left", "another", frame)).toBe(false);
  });

  test("with-panels falls back to the first panel when selection is stale", () => {
    const pane = createCanvasPaneWithPanels(
      "pane",
      canvasRect(0, 0, 10, 10),
      ["a", "b"],
      "missing",
    );
    expect(pane.selectedPanelId).toBe("a");
  });
});
