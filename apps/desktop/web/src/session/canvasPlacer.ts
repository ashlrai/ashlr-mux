import {
  CANVAS_ZERO_POINT,
  DEFAULT_CANVAS_METRICS,
  canvasRect,
  canvasRectFromOriginSize,
  canvasRectOrigin,
  canvasRectsIntersect,
  expandedCanvasRect,
  maxX,
  maxY,
  minX,
  minY,
  unionCanvasRects,
  type CanvasDirection,
  type CanvasMetrics,
  type CanvasRect,
  type CanvasSize,
} from "./canvasGeometry";

const SCAN_COLUMNS = 16;
const SCAN_ROWS = 8;

export function frameForNewCanvasPane(
  size: CanvasSize,
  anchor: CanvasRect | null | undefined,
  existing: readonly CanvasRect[],
  preferredDirection?: CanvasDirection | null,
  metrics: CanvasMetrics = DEFAULT_CANVAS_METRICS,
): CanvasRect {
  const gap = metrics.gap;
  if (existing.length === 0) {
    return canvasRectFromOriginSize(
      anchor === null || anchor === undefined
        ? CANVAS_ZERO_POINT
        : canvasRectOrigin(anchor),
      size,
    );
  }
  if (anchor === null || anchor === undefined) {
    return frameRightOfContent(size, existing, metrics);
  }

  const neighbors: Array<{ direction: CanvasDirection; frame: CanvasRect }> = [
    {
      direction: "right",
      frame: canvasRect(maxX(anchor) + gap, minY(anchor), size.width, size.height),
    },
    {
      direction: "down",
      frame: canvasRect(minX(anchor), maxY(anchor) + gap, size.width, size.height),
    },
    {
      direction: "left",
      frame: canvasRect(
        minX(anchor) - gap - size.width,
        minY(anchor),
        size.width,
        size.height,
      ),
    },
    {
      direction: "up",
      frame: canvasRect(
        minX(anchor),
        minY(anchor) - gap - size.height,
        size.width,
        size.height,
      ),
    },
  ];
  const preferred = neighbors.find(
    (neighbor) => neighbor.direction === preferredDirection,
  );
  const ordered =
    preferred === undefined
      ? neighbors
      : [
          preferred,
          ...neighbors.filter((neighbor) => neighbor !== preferred),
        ];

  for (const candidate of ordered) {
    if (isFree(candidate.frame, existing, metrics)) {
      return candidate.frame;
    }
  }

  for (let row = 0; row < SCAN_ROWS; row += 1) {
    const y = minY(anchor) + row * (size.height + gap);
    for (let column = 0; column < SCAN_COLUMNS; column += 1) {
      const x = maxX(anchor) + gap + column * (size.width + gap);
      const candidate = canvasRect(x, y, size.width, size.height);
      if (isFree(candidate, existing, metrics)) {
        return candidate;
      }
    }
  }
  return frameRightOfContent(size, existing, metrics);
}

function isFree(
  candidate: CanvasRect,
  existing: readonly CanvasRect[],
  metrics: CanvasMetrics,
): boolean {
  const probe = expandedCanvasRect(candidate, metrics.gap - 0.5);
  return !existing.some((rect) => canvasRectsIntersect(probe, rect));
}

function frameRightOfContent(
  size: CanvasSize,
  existing: readonly CanvasRect[],
  metrics: CanvasMetrics,
): CanvasRect {
  const bounds = existing
    .slice(1)
    .reduce((acc, rect) => unionCanvasRects(acc, rect), existing[0]);
  return canvasRect(
    maxX(bounds) + metrics.gap,
    minY(bounds),
    size.width,
    size.height,
  );
}
