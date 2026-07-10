import {
  canvasRectContainsPoint,
  unionCanvasRects,
  type CanvasPoint,
  type CanvasRect,
} from "./canvasGeometry";

export type CanvasPaneId = string;
export type CanvasPanelId = string;

export interface CanvasPane {
  id: CanvasPaneId;
  frame: CanvasRect;
  panelIds: CanvasPanelId[];
  selectedPanelId: CanvasPanelId;
}

export function createCanvasPane(
  id: CanvasPaneId,
  frame: CanvasRect,
): CanvasPane {
  return createCanvasPaneWithPanels(id, frame, [id], id);
}

export function createCanvasPaneWithPanels(
  id: CanvasPaneId,
  frame: CanvasRect,
  panelIds: readonly CanvasPanelId[],
  selectedPanelId: CanvasPanelId,
): CanvasPane {
  if (panelIds.length === 0) {
    throw new Error("A canvas pane must host at least one panel");
  }
  const panels = [...panelIds];
  return {
    id,
    frame,
    panelIds: panels,
    selectedPanelId: panels.includes(selectedPanelId) ? selectedPanelId : panels[0]!,
  };
}

export class CanvasLayout {
  private panesValue: CanvasPane[];

  constructor(panes: readonly CanvasPane[] = []) {
    this.panesValue = panes.map(clonePane);
  }

  panes(): CanvasPane[] {
    return this.panesValue.map(clonePane);
  }

  paneIds(): CanvasPaneId[] {
    return this.panesValue.map((pane) => pane.id);
  }

  isEmpty(): boolean {
    return this.panesValue.length === 0;
  }

  frame(id: CanvasPaneId): CanvasRect | undefined {
    return this.panesValue.find((pane) => pane.id === id)?.frame;
  }

  contains(id: CanvasPaneId): boolean {
    return this.panesValue.some((pane) => pane.id === id);
  }

  framesExcluding(excluded: CanvasPaneId): CanvasRect[] {
    return this.panesValue
      .filter((pane) => pane.id !== excluded)
      .map((pane) => pane.frame);
  }

  contentBounds(): CanvasRect | undefined {
    const first = this.panesValue[0];
    if (first === undefined) {
      return undefined;
    }
    return this.panesValue
      .slice(1)
      .reduce((acc, pane) => unionCanvasRects(acc, pane.frame), first.frame);
  }

  topPane(point: CanvasPoint): CanvasPaneId | undefined {
    for (let index = this.panesValue.length - 1; index >= 0; index -= 1) {
      const pane = this.panesValue[index]!;
      if (canvasRectContainsPoint(pane.frame, point)) {
        return pane.id;
      }
    }
    return undefined;
  }

  add(pane: CanvasPane): void {
    this.panesValue = this.panesValue.filter((existing) => existing.id !== pane.id);
    this.panesValue.push(clonePane(pane));
  }

  remove(id: CanvasPaneId): void {
    this.panesValue = this.panesValue.filter((pane) => pane.id !== id);
  }

  setFrame(frame: CanvasRect, id: CanvasPaneId): void {
    const pane = this.panesValue.find((candidate) => candidate.id === id);
    if (pane !== undefined) {
      pane.frame = frame;
    }
  }

  setFrames(frames: ReadonlyMap<CanvasPaneId, CanvasRect>): void {
    for (const pane of this.panesValue) {
      const frame = frames.get(pane.id);
      if (frame !== undefined) {
        pane.frame = frame;
      }
    }
  }

  bringToFront(id: CanvasPaneId): void {
    const index = this.panesValue.findIndex((pane) => pane.id === id);
    if (index < 0 || index === this.panesValue.length - 1) {
      return;
    }
    const [pane] = this.panesValue.splice(index, 1);
    if (pane !== undefined) {
      this.panesValue.push(pane);
    }
  }

  allPanelIds(): CanvasPanelId[] {
    return this.panesValue.flatMap((pane) => pane.panelIds);
  }

  paneContaining(panelId: CanvasPanelId): CanvasPaneId | undefined {
    return this.panesValue.find((pane) => pane.panelIds.includes(panelId))?.id;
  }

  panelIdsIn(id: CanvasPaneId): CanvasPanelId[] | undefined {
    const pane = this.panesValue.find((candidate) => candidate.id === id);
    return pane === undefined ? undefined : [...pane.panelIds];
  }

  selectedPanelIdIn(id: CanvasPaneId): CanvasPanelId | undefined {
    return this.panesValue.find((pane) => pane.id === id)?.selectedPanelId;
  }

  selectPanel(panelId: CanvasPanelId): void {
    const pane = this.panesValue.find((candidate) =>
      candidate.panelIds.includes(panelId),
    );
    if (pane !== undefined) {
      pane.selectedPanelId = panelId;
    }
  }

  addPanel(
    panelId: CanvasPanelId,
    toPane: CanvasPaneId,
    index?: number | null,
    select = false,
  ): void {
    if (!this.contains(toPane)) {
      return;
    }
    if (this.paneContaining(panelId) !== toPane) {
      this.removePanel(panelId);
    }
    const pane = this.panesValue.find((candidate) => candidate.id === toPane);
    if (pane === undefined) {
      return;
    }
    if (!pane.panelIds.includes(panelId)) {
      const clamped = Math.max(
        0,
        Math.min(index ?? pane.panelIds.length, pane.panelIds.length),
      );
      pane.panelIds.splice(clamped, 0, panelId);
    }
    if (select) {
      pane.selectedPanelId = panelId;
    }
  }

  removePanel(panelId: CanvasPanelId): CanvasPaneId | undefined {
    const paneIndex = this.panesValue.findIndex((pane) =>
      pane.panelIds.includes(panelId),
    );
    if (paneIndex < 0) {
      return undefined;
    }
    const pane = this.panesValue[paneIndex]!;
    const panelIndex = pane.panelIds.indexOf(panelId);
    if (pane.panelIds.length <= 1) {
      this.panesValue.splice(paneIndex, 1);
      return pane.id;
    }
    pane.panelIds.splice(panelIndex, 1);
    if (pane.selectedPanelId === panelId) {
      pane.selectedPanelId =
        pane.panelIds[Math.min(panelIndex, pane.panelIds.length - 1)]!;
    }
    return pane.id;
  }

  breakOutPanel(
    panelId: CanvasPanelId,
    newPaneId: CanvasPaneId,
    frame: CanvasRect,
  ): boolean {
    const pane = this.panesValue.find((candidate) =>
      candidate.panelIds.includes(panelId),
    );
    if (
      pane === undefined ||
      pane.panelIds.length <= 1 ||
      this.contains(newPaneId)
    ) {
      return false;
    }
    const removedFrom = this.removePanel(panelId);
    if (removedFrom === undefined) {
      return false;
    }
    this.panesValue.push(createCanvasPaneWithPanels(
      newPaneId,
      frame,
      [panelId],
      panelId,
    ));
    return true;
  }
}

function clonePane(pane: CanvasPane): CanvasPane {
  return {
    id: pane.id,
    frame: { ...pane.frame },
    panelIds: [...pane.panelIds],
    selectedPanelId: pane.selectedPanelId,
  };
}
