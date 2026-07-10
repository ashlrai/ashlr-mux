import { describe, expect, test } from "bun:test";

import {
  snapCanvasFrameForMove,
  snapCanvasFrameForResize,
  type CanvasFrame,
} from "./canvasSnap";

const neighbor: CanvasFrame = { x: 0, y: 0, width: 300, height: 200 };

describe("snapCanvasFrameForMove", () => {
  test("snaps a moved pane edge to a neighbor edge", () => {
    const result = snapCanvasFrameForMove(
      { x: 5, y: 400, width: 300, height: 200 },
      [neighbor],
    );
    expect(result.frame.x).toBe(0);
    expect(result.frame.y).toBe(400);
    expect(result.guides).toEqual([
      { axis: "vertical", position: 0, start: 0, end: 600 },
    ]);
  });

  test("snaps to the canonical gap next to a neighbor", () => {
    const result = snapCanvasFrameForMove(
      { x: 312, y: 500, width: 300, height: 200 },
      [neighbor],
    );
    expect(result.frame.x).toBe(316);
    expect(result.guides[0]?.position).toBe(316);
  });

  test("uses a configured pane gap for adjacency snapping", () => {
    const result = snapCanvasFrameForMove(
      { x: 318, y: 500, width: 300, height: 200 },
      [neighbor],
      {
        gap: 24,
        snapThreshold: 8,
        minPaneSize: { width: 200, height: 120 },
      },
    );
    expect(result.frame.x).toBe(324);
    expect(result.guides[0]?.position).toBe(324);
  });

  test("snaps both axes independently", () => {
    const result = snapCanvasFrameForMove(
      { x: 314, y: 3, width: 300, height: 200 },
      [neighbor],
    );
    expect(result.frame.x).toBe(316);
    expect(result.frame.y).toBe(0);
    expect(result.guides.map((guide) => guide.axis).sort()).toEqual([
      "horizontal",
      "vertical",
    ]);
  });

  test("does not snap beyond the threshold", () => {
    const proposed = { x: 9, y: 400, width: 300, height: 200 };
    const result = snapCanvasFrameForMove(proposed, [neighbor]);
    expect(result.frame).toEqual(proposed);
    expect(result.guides).toEqual([]);
  });

  test("prefers edge alignment over center alignment on ties", () => {
    const result = snapCanvasFrameForMove(
      { x: 98, y: 300, width: 100, height: 100 },
      [{ x: 100, y: 0, width: 100, height: 100 }],
    );
    expect(result.frame.x).toBe(100);
    expect(result.guides[0]?.position).toBe(100);
  });
});

describe("snapCanvasFrameForResize", () => {
  test("snaps a right resize edge to a neighbor right edge", () => {
    const result = snapCanvasFrameForResize(
      { x: 0, y: 400, width: 295, height: 200 },
      { right: true },
      [neighbor],
    );
    expect(result.frame).toEqual({ x: 0, y: 400, width: 300, height: 200 });
    expect(result.guides[0]?.position).toBe(300);
  });

  test("snaps a left resize edge to the canonical gap beside a neighbor", () => {
    const result = snapCanvasFrameForResize(
      { x: 320, y: 0, width: 300, height: 200 },
      { left: true },
      [neighbor],
    );
    expect(result.frame.x).toBe(316);
    expect(result.frame.x + result.frame.width).toBe(620);
  });

  test("clamps resize frames to the shared minimum pane size", () => {
    const result = snapCanvasFrameForResize(
      { x: 0, y: 0, width: 150, height: 80 },
      { right: true, bottom: true },
      [],
    );
    expect(result.frame).toEqual({ x: 0, y: 0, width: 200, height: 120 });
  });

  test("drops a guide when minimum-size clamping undoes its snapped edge", () => {
    const result = snapCanvasFrameForResize(
      { x: 400, y: 0, width: 150, height: 200 },
      { left: true },
      [{ x: 395, y: 0, width: 100, height: 100 }],
    );
    expect(result.frame.width).toBe(200);
    expect(result.frame.x + result.frame.width).toBe(550);
    expect(result.guides).toEqual([]);
  });
});
