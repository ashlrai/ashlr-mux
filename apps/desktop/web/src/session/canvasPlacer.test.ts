import { describe, expect, test } from "bun:test";

import {
  DEFAULT_CANVAS_METRICS,
  canvasPoint,
  canvasRect,
  canvasRectOrigin,
  canvasRectSize,
  canvasRectsIntersect,
  expandedCanvasRect,
} from "./canvasGeometry";
import { frameForNewCanvasPane } from "./canvasPlacer";

const size = canvasRectSize(canvasRect(0, 0, 300, 200));

describe("frameForNewCanvasPane", () => {
  test("empty canvas places at origin or anchor", () => {
    expect(frameForNewCanvasPane(size, undefined, [])).toEqual(
      canvasRect(0, 0, 300, 200),
    );
    const anchor = canvasRect(50, 60, 10, 10);
    expect(canvasRectOrigin(frameForNewCanvasPane(size, anchor, []))).toEqual(
      canvasPoint(50, 60),
    );
  });

  test("places right of anchor at the canonical gap", () => {
    const anchor = canvasRect(0, 0, 300, 200);
    expect(frameForNewCanvasPane(size, anchor, [anchor])).toEqual(
      canvasRect(316, 0, 300, 200),
    );
  });

  test("honors preferred direction when that slot is free", () => {
    const anchor = canvasRect(1000, 1000, 300, 200);
    expect(frameForNewCanvasPane(size, anchor, [anchor], "down")).toEqual(
      canvasRect(1000, 1216, 300, 200),
    );
  });

  test("falls through right, below, left, then above", () => {
    const anchor = canvasRect(1000, 1000, 300, 200);
    const right = canvasRect(1316, 1000, 300, 200);
    const below = canvasRect(1000, 1216, 300, 200);
    expect(frameForNewCanvasPane(size, anchor, [anchor, right, below])).toEqual(
      canvasRect(684, 1000, 300, 200),
    );
  });

  test("new pane keeps gap distance from all panes", () => {
    const anchor = canvasRect(0, 0, 300, 200);
    const blocker = canvasRect(320, 10, 300, 200);
    const frame = frameForNewCanvasPane(size, anchor, [anchor, blocker]);
    for (const existing of [anchor, blocker]) {
      expect(canvasRectsIntersect(
        expandedCanvasRect(frame, DEFAULT_CANVAS_METRICS.gap - 0.5),
        existing,
      )).toBe(false);
    }
  });

  test("no anchor places right of content", () => {
    const existing = [
      canvasRect(0, 40, 300, 200),
      canvasRect(350, 0, 300, 200),
    ];
    expect(frameForNewCanvasPane(size, undefined, existing)).toEqual(
      canvasRect(666, 0, 300, 200),
    );
  });
});
