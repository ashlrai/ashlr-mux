import { describe, expect, test } from "bun:test";

import {
  canvasPoint,
  canvasRect,
  canvasRectCenter,
  canvasRectContainsPoint,
  canvasRectsIntersect,
  expandedCanvasRect,
  offsetCanvasRect,
  unionCanvasRects,
  maxX,
  maxY,
  midX,
  midY,
  minX,
  minY,
} from "./canvasGeometry";

describe("canvas geometry", () => {
  test("projects edges and center", () => {
    const rect = canvasRect(10, 20, 100, 50);
    expect(minX(rect)).toBe(10);
    expect(maxX(rect)).toBe(110);
    expect(midX(rect)).toBe(60);
    expect(minY(rect)).toBe(20);
    expect(maxY(rect)).toBe(70);
    expect(midY(rect)).toBe(45);
    expect(canvasRectCenter(rect)).toEqual(canvasPoint(60, 45));
  });

  test("offset and expand mirror canvas rect math", () => {
    const rect = canvasRect(0, 0, 10, 10);
    expect(offsetCanvasRect(rect, 5, -5)).toEqual(canvasRect(5, -5, 10, 10));
    expect(expandedCanvasRect(rect, 2)).toEqual(canvasRect(-2, -2, 14, 14));
    expect(expandedCanvasRect(rect, -2)).toEqual(canvasRect(2, 2, 6, 6));
  });

  test("intersection requires positive area", () => {
    const rect = canvasRect(0, 0, 10, 10);
    expect(canvasRectsIntersect(rect, canvasRect(5, 5, 10, 10))).toBe(true);
    expect(canvasRectsIntersect(rect, canvasRect(10, 0, 10, 10))).toBe(false);
    expect(canvasRectsIntersect(rect, canvasRect(0, 10, 10, 10))).toBe(false);
    expect(canvasRectsIntersect(rect, canvasRect(20, 20, 5, 5))).toBe(false);
  });

  test("contains is closed on min edges and open on max edges", () => {
    const rect = canvasRect(0, 0, 10, 10);
    expect(canvasRectContainsPoint(rect, canvasPoint(0, 0))).toBe(true);
    expect(canvasRectContainsPoint(rect, canvasPoint(9.99, 9.99))).toBe(true);
    expect(canvasRectContainsPoint(rect, canvasPoint(10, 5))).toBe(false);
    expect(canvasRectContainsPoint(rect, canvasPoint(5, 10))).toBe(false);
    expect(canvasRectContainsPoint(rect, canvasPoint(-0.01, 5))).toBe(false);
  });

  test("union covers both rects", () => {
    expect(unionCanvasRects(
      canvasRect(0, 0, 10, 10),
      canvasRect(30, -5, 5, 5),
    )).toEqual(canvasRect(0, -5, 35, 15));
  });
});
