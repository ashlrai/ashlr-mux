export interface CanvasFrame {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CanvasSize {
  width: number;
  height: number;
}

export interface CanvasSnapMetrics {
  gap: number;
  snapThreshold: number;
  minPaneSize: CanvasSize;
}

export interface CanvasResizeEdges {
  top?: boolean;
  right?: boolean;
  bottom?: boolean;
  left?: boolean;
}

export interface CanvasGuide {
  axis: "vertical" | "horizontal";
  position: number;
  start: number;
  end: number;
}

export interface CanvasSnapResult<TFrame extends CanvasFrame = CanvasFrame> {
  frame: TFrame;
  guides: CanvasGuide[];
}

interface Candidate {
  delta: number;
  guidePosition: number;
  priority: number;
  neighbor: CanvasFrame;
}

export const DEFAULT_CANVAS_SNAP_METRICS: CanvasSnapMetrics = {
  gap: 16,
  snapThreshold: 8,
  minPaneSize: { width: 200, height: 120 },
};

export function snapCanvasFrameForMove<TFrame extends CanvasFrame>(
  proposed: TFrame,
  neighbors: readonly CanvasFrame[],
  metrics: CanvasSnapMetrics = DEFAULT_CANVAS_SNAP_METRICS,
): CanvasSnapResult<TFrame> {
  const frame = { ...proposed };
  const guides: CanvasGuide[] = [];
  const bestX = bestCandidate(moveCandidatesX(proposed, neighbors, metrics), metrics);
  if (bestX !== undefined) {
    frame.x = roundCanvas(frame.x + bestX.delta);
    guides.push(verticalGuide(bestX.guidePosition, frame, bestX.neighbor));
  }
  const bestY = bestCandidate(moveCandidatesY(proposed, neighbors, metrics), metrics);
  if (bestY !== undefined) {
    frame.y = roundCanvas(frame.y + bestY.delta);
    guides.push(horizontalGuide(bestY.guidePosition, frame, bestY.neighbor));
  }
  return { frame, guides };
}

export function snapCanvasFrameForResize<TFrame extends CanvasFrame>(
  proposed: TFrame,
  edges: CanvasResizeEdges,
  neighbors: readonly CanvasFrame[],
  metrics: CanvasSnapMetrics = DEFAULT_CANVAS_SNAP_METRICS,
): CanvasSnapResult<TFrame> {
  const frame = { ...proposed };
  const guides: CanvasGuide[] = [];

  if (edges.left === true) {
    const best = bestCandidate(
      edgeCandidates(
        minX(proposed),
        neighbors.map((neighbor) => [minX(neighbor), neighbor]),
        neighbors.map((neighbor) => [maxX(neighbor) + metrics.gap, neighbor]),
      ),
      metrics,
    );
    if (best !== undefined) {
      frame.x = roundCanvas(minX(proposed) + best.delta);
      frame.width = roundCanvas(maxX(proposed) - frame.x);
      guides.push(verticalGuide(best.guidePosition, frame, best.neighbor));
    }
    if (frame.width < metrics.minPaneSize.width) {
      frame.x = roundCanvas(maxX(frame) - metrics.minPaneSize.width);
      frame.width = metrics.minPaneSize.width;
      removeAxisGuides(guides, "vertical");
    }
  } else if (edges.right === true) {
    const best = bestCandidate(
      edgeCandidates(
        maxX(proposed),
        neighbors.map((neighbor) => [maxX(neighbor), neighbor]),
        neighbors.map((neighbor) => [minX(neighbor) - metrics.gap, neighbor]),
      ),
      metrics,
    );
    if (best !== undefined) {
      frame.width = roundCanvas(maxX(proposed) + best.delta - frame.x);
      guides.push(verticalGuide(best.guidePosition, frame, best.neighbor));
    }
    if (frame.width < metrics.minPaneSize.width) {
      frame.width = metrics.minPaneSize.width;
      removeAxisGuides(guides, "vertical");
    }
  }

  if (edges.top === true) {
    const best = bestCandidate(
      edgeCandidates(
        minY(proposed),
        neighbors.map((neighbor) => [minY(neighbor), neighbor]),
        neighbors.map((neighbor) => [maxY(neighbor) + metrics.gap, neighbor]),
      ),
      metrics,
    );
    if (best !== undefined) {
      frame.y = roundCanvas(minY(proposed) + best.delta);
      frame.height = roundCanvas(maxY(proposed) - frame.y);
      guides.push(horizontalGuide(best.guidePosition, frame, best.neighbor));
    }
    if (frame.height < metrics.minPaneSize.height) {
      frame.y = roundCanvas(maxY(frame) - metrics.minPaneSize.height);
      frame.height = metrics.minPaneSize.height;
      removeAxisGuides(guides, "horizontal");
    }
  } else if (edges.bottom === true) {
    const best = bestCandidate(
      edgeCandidates(
        maxY(proposed),
        neighbors.map((neighbor) => [maxY(neighbor), neighbor]),
        neighbors.map((neighbor) => [minY(neighbor) - metrics.gap, neighbor]),
      ),
      metrics,
    );
    if (best !== undefined) {
      frame.height = roundCanvas(maxY(proposed) + best.delta - frame.y);
      guides.push(horizontalGuide(best.guidePosition, frame, best.neighbor));
    }
    if (frame.height < metrics.minPaneSize.height) {
      frame.height = metrics.minPaneSize.height;
      removeAxisGuides(guides, "horizontal");
    }
  }

  return { frame, guides };
}

function moveCandidatesX(
  rect: CanvasFrame,
  neighbors: readonly CanvasFrame[],
  metrics: CanvasSnapMetrics,
): Candidate[] {
  return neighbors.flatMap((neighbor) => [
    candidate(minX(neighbor) - minX(rect), minX(neighbor), 0, neighbor),
    candidate(maxX(neighbor) - maxX(rect), maxX(neighbor), 0, neighbor),
    candidate(maxX(neighbor) + metrics.gap - minX(rect), maxX(neighbor) + metrics.gap, 1, neighbor),
    candidate(minX(neighbor) - metrics.gap - maxX(rect), minX(neighbor) - metrics.gap, 1, neighbor),
    candidate(midX(neighbor) - midX(rect), midX(neighbor), 2, neighbor),
  ]);
}

function moveCandidatesY(
  rect: CanvasFrame,
  neighbors: readonly CanvasFrame[],
  metrics: CanvasSnapMetrics,
): Candidate[] {
  return neighbors.flatMap((neighbor) => [
    candidate(minY(neighbor) - minY(rect), minY(neighbor), 0, neighbor),
    candidate(maxY(neighbor) - maxY(rect), maxY(neighbor), 0, neighbor),
    candidate(maxY(neighbor) + metrics.gap - minY(rect), maxY(neighbor) + metrics.gap, 1, neighbor),
    candidate(minY(neighbor) - metrics.gap - maxY(rect), minY(neighbor) - metrics.gap, 1, neighbor),
    candidate(midY(neighbor) - midY(rect), midY(neighbor), 2, neighbor),
  ]);
}

function edgeCandidates(
  edge: number,
  alignTargets: Array<[number, CanvasFrame]>,
  gapTargets: Array<[number, CanvasFrame]>,
): Candidate[] {
  return [
    ...alignTargets.map(([target, neighbor]) =>
      candidate(target - edge, target, 0, neighbor),
    ),
    ...gapTargets.map(([target, neighbor]) =>
      candidate(target - edge, target, 1, neighbor),
    ),
  ];
}

function bestCandidate(
  candidates: readonly Candidate[],
  metrics: CanvasSnapMetrics,
): Candidate | undefined {
  let best: Candidate | undefined;
  for (const candidate of candidates) {
    if (Math.abs(candidate.delta) > metrics.snapThreshold) {
      continue;
    }
    if (
      best === undefined ||
      Math.abs(candidate.delta) < Math.abs(best.delta) ||
      (Math.abs(candidate.delta) === Math.abs(best.delta) &&
        candidate.priority < best.priority)
    ) {
      best = candidate;
    }
  }
  return best;
}

function candidate(
  delta: number,
  guidePosition: number,
  priority: number,
  neighbor: CanvasFrame,
): Candidate {
  return { delta, guidePosition, priority, neighbor };
}

function verticalGuide(
  position: number,
  snapped: CanvasFrame,
  neighbor: CanvasFrame,
): CanvasGuide {
  const lower = Math.min(minY(snapped), minY(neighbor));
  const upper = Math.max(maxY(snapped), maxY(neighbor));
  return { axis: "vertical", position, start: lower, end: Math.max(lower, upper) };
}

function horizontalGuide(
  position: number,
  snapped: CanvasFrame,
  neighbor: CanvasFrame,
): CanvasGuide {
  const lower = Math.min(minX(snapped), minX(neighbor));
  const upper = Math.max(maxX(snapped), maxX(neighbor));
  return { axis: "horizontal", position, start: lower, end: Math.max(lower, upper) };
}

function removeAxisGuides(guides: CanvasGuide[], axis: CanvasGuide["axis"]): void {
  for (let index = guides.length - 1; index >= 0; index -= 1) {
    if (guides[index]?.axis === axis) {
      guides.splice(index, 1);
    }
  }
}

function roundCanvas(value: number): number {
  return Math.round(value);
}

function minX(frame: CanvasFrame): number {
  return frame.x;
}

function maxX(frame: CanvasFrame): number {
  return frame.x + frame.width;
}

function midX(frame: CanvasFrame): number {
  return frame.x + frame.width / 2;
}

function minY(frame: CanvasFrame): number {
  return frame.y;
}

function maxY(frame: CanvasFrame): number {
  return frame.y + frame.height;
}

function midY(frame: CanvasFrame): number {
  return frame.y + frame.height / 2;
}
