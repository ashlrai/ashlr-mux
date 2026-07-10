export interface CanvasPoint {
  x: number;
  y: number;
}

export interface CanvasSize {
  width: number;
  height: number;
}

export interface CanvasRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CanvasDirection = "left" | "right" | "up" | "down";

export interface CanvasMetrics {
  gap: number;
  snapThreshold: number;
  minPaneSize: CanvasSize;
}

export const CANVAS_ZERO_POINT: CanvasPoint = { x: 0, y: 0 };
export const CANVAS_ZERO_SIZE: CanvasSize = { width: 0, height: 0 };
export const CANVAS_ZERO_RECT: CanvasRect = {
  x: 0,
  y: 0,
  width: 0,
  height: 0,
};

export const DEFAULT_CANVAS_METRICS: CanvasMetrics = {
  gap: 16,
  snapThreshold: 8,
  minPaneSize: { width: 200, height: 120 },
};

export function canvasPoint(x: number, y: number): CanvasPoint {
  return { x, y };
}

export function canvasSize(width: number, height: number): CanvasSize {
  return { width, height };
}

export function canvasRect(
  x: number,
  y: number,
  width: number,
  height: number,
): CanvasRect {
  return { x, y, width, height };
}

export function canvasRectFromOriginSize(
  origin: CanvasPoint,
  size: CanvasSize,
): CanvasRect {
  return canvasRect(origin.x, origin.y, size.width, size.height);
}

export function minX(rect: CanvasRect): number {
  return rect.x;
}

export function maxX(rect: CanvasRect): number {
  return rect.x + rect.width;
}

export function midX(rect: CanvasRect): number {
  return rect.x + rect.width / 2;
}

export function minY(rect: CanvasRect): number {
  return rect.y;
}

export function maxY(rect: CanvasRect): number {
  return rect.y + rect.height;
}

export function midY(rect: CanvasRect): number {
  return rect.y + rect.height / 2;
}

export function canvasRectOrigin(rect: CanvasRect): CanvasPoint {
  return canvasPoint(rect.x, rect.y);
}

export function canvasRectSize(rect: CanvasRect): CanvasSize {
  return canvasSize(rect.width, rect.height);
}

export function canvasRectCenter(rect: CanvasRect): CanvasPoint {
  return canvasPoint(midX(rect), midY(rect));
}

export function offsetCanvasRect(
  rect: CanvasRect,
  dx: number,
  dy: number,
): CanvasRect {
  return canvasRect(rect.x + dx, rect.y + dy, rect.width, rect.height);
}

export function expandedCanvasRect(rect: CanvasRect, amount: number): CanvasRect {
  return canvasRect(
    rect.x - amount,
    rect.y - amount,
    rect.width + amount * 2,
    rect.height + amount * 2,
  );
}

export function insetCanvasRect(
  rect: CanvasRect,
  dx: number,
  dy: number,
): CanvasRect {
  return canvasRect(
    rect.x + dx,
    rect.y + dy,
    rect.width - dx * 2,
    rect.height - dy * 2,
  );
}

export function canvasRectsIntersect(lhs: CanvasRect, rhs: CanvasRect): boolean {
  return (
    minX(lhs) < maxX(rhs) &&
    minX(rhs) < maxX(lhs) &&
    minY(lhs) < maxY(rhs) &&
    minY(rhs) < maxY(lhs)
  );
}

export function canvasRectContainsPoint(
  rect: CanvasRect,
  point: CanvasPoint,
): boolean {
  return (
    point.x >= minX(rect) &&
    point.x < maxX(rect) &&
    point.y >= minY(rect) &&
    point.y < maxY(rect)
  );
}

export function unionCanvasRects(lhs: CanvasRect, rhs: CanvasRect): CanvasRect {
  const x = Math.min(minX(lhs), minX(rhs));
  const y = Math.min(minY(lhs), minY(rhs));
  return canvasRect(
    x,
    y,
    Math.max(maxX(lhs), maxX(rhs)) - x,
    Math.max(maxY(lhs), maxY(rhs)) - y,
  );
}

export function canvasRectContainsRect(
  outer: CanvasRect,
  inner: CanvasRect,
): boolean {
  return (
    minX(inner) >= minX(outer) &&
    maxX(inner) <= maxX(outer) &&
    minY(inner) >= minY(outer) &&
    maxY(inner) <= maxY(outer)
  );
}
