import { useSyncExternalStore } from "react";

import type { SessionCanvasPaneSnapshot } from "@cmux/core-types";

import { paneRects, type Rect } from "./paneRects";
import { firstActivePanelId, isPane, type Layout } from "./splitLayout";

/**
 * C4 — focused-pane tracking, the web-side source of truth for "which panel
 * owns keyboard focus".
 *
 * Canonical macOS derives this from the bonsplit tree's focused pane surfaced
 * as a panel id (`WorkspaceSurfaceListModel.focusedPanelId`,
 * Packages/macOS/CmuxWorkspaces/.../WorkspaceSurfaceListModel.swift:61-67),
 * fed by pane tap gestures and the AppKit first-responder sync
 * (WorkspaceContentView.swift:215-217 / 231-238). The port's generated session
 * model persists `focused_panel_id`; Workspace mirrors that value into this
 * low-latency store and writes capture-phase pointer/focus changes back through
 * the session bridge. The command palette can therefore read synchronously
 * while CLI `pane.last` and restored sessions share the same focus truth.
 */

export interface FocusedPaneStore {
  focus(panelId: string): void;
  clear(): void;
  get(): string | undefined;
  subscribe(listener: () => void): () => void;
}

/**
 * A last-focused-panel cell. Listeners are notified only on CHANGE — pane
 * wrappers re-fire pointer/focus events constantly (every click, every xterm
 * refocus), and a same-id write must never re-render the palette host.
 */
export function createFocusedPaneStore(): FocusedPaneStore {
  let current: string | undefined;
  const listeners = new Set<() => void>();
  const notify = (): void => {
    for (const listener of listeners) {
      listener();
    }
  };
  return {
    focus(panelId: string): void {
      if (panelId !== current) {
        current = panelId;
        notify();
      }
    },
    clear(): void {
      if (current !== undefined) {
        current = undefined;
        notify();
      }
    },
    get: () => current,
    subscribe(listener: () => void): () => void {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app singleton: `Workspace` writes it, `useCommandPalette` reads it. */
export const focusedPaneStore = createFocusedPaneStore();

/** Every panel id hosted by any leaf pane of `layout` (all `panel_ids`). */
export function panelIdsInLayout(layout: Layout): ReadonlySet<string> {
  const ids = new Set<string>();
  const walk = (node: Layout): void => {
    if (isPane(node)) {
      for (const id of node.pane.panel_ids) {
        ids.add(id);
      }
      return;
    }
    walk(node.split.first);
    walk(node.split.second);
  };
  walk(layout);
  return ids;
}

/** The pane id that owns `panelId`, or undefined when the layout lacks one. */
export function paneIdForPanelId(
  layout: Layout,
  panelId: string,
): string | undefined {
  if (isPane(layout)) {
    return layout.pane.panel_ids.includes(panelId) ||
      layout.pane.selected_panel_id === panelId
      ? layout.pane.pane_id
      : undefined;
  }
  return (
    paneIdForPanelId(layout.split.first, panelId) ??
    paneIdForPanelId(layout.split.second, panelId)
  );
}

/**
 * The focused panel id revalidated against the CURRENT layout, with a
 * first-leaf fallback — mirroring the canonical session-restore
 * validate-then-fallback (Workspace.swift:242-246) and the
 * `performSplitShortcut` workspace-default fallback (AppDelegate.swift:14545).
 *
 * The store deliberately keeps the RAW last-focused id even when it is absent
 * from the current workspace's layout: resolution revalidates per read, so
 * switching workspaces falls back to that workspace's first leaf and switching
 * back restores the remembered focus (cheap per-workspace focus memory; panel
 * ids are globally unique, so a stale id can never collide with another
 * workspace's pane).
 */
export function resolveActivePanelId(
  focused: string | undefined,
  layout: Layout | null | undefined,
): string | undefined {
  if (!layout) {
    return undefined;
  }
  if (focused !== undefined && panelIdsInLayout(layout).has(focused)) {
    return focused;
  }
  return firstActivePanelId(layout);
}

/** The focused pane id revalidated against the current layout, else first-leaf. */
export function resolveActivePaneId(
  focused: string | undefined,
  layout: Layout | null | undefined,
): string | undefined {
  if (!layout) {
    return undefined;
  }
  const panelId = resolveActivePanelId(focused, layout);
  return panelId === undefined ? undefined : paneIdForPanelId(layout, panelId);
}

export type PaneFocusDirection = "left" | "right" | "up" | "down";

function center(rect: Rect): { x: number; y: number } {
  return { x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 };
}

function intervalGap(a0: number, a1: number, b0: number, b1: number): number {
  if (a1 < b0) {
    return b0 - a1;
  }
  if (b1 < a0) {
    return a0 - b1;
  }
  return 0;
}

function primaryGap(
  current: Rect,
  candidate: Rect,
  direction: PaneFocusDirection,
): number {
  switch (direction) {
    case "left":
      return Math.max(0, current.x - (candidate.x + candidate.w));
    case "right":
      return Math.max(0, candidate.x - (current.x + current.w));
    case "up":
      return Math.max(0, current.y - (candidate.y + candidate.h));
    case "down":
      return Math.max(0, candidate.y - (current.y + current.h));
  }
}

function orthogonalGap(
  current: Rect,
  candidate: Rect,
  direction: PaneFocusDirection,
): number {
  if (direction === "left" || direction === "right") {
    return intervalGap(
      current.y,
      current.y + current.h,
      candidate.y,
      candidate.y + candidate.h,
    );
  }
  return intervalGap(
    current.x,
    current.x + current.w,
    candidate.x,
    candidate.x + candidate.w,
  );
}

function isInDirection(
  current: Rect,
  candidate: Rect,
  direction: PaneFocusDirection,
): boolean {
  const c = center(current);
  const n = center(candidate);
  switch (direction) {
    case "left":
      return n.x < c.x;
    case "right":
      return n.x > c.x;
    case "up":
      return n.y < c.y;
    case "down":
      return n.y > c.y;
  }
}

/**
 * Resolve the pane that should receive focus for a directional focus command.
 * Geometry comes from the same flat-portal rectangles used by Workspace, so the
 * command path and renderer agree on what "left/right/up/down" means.
 */
export function adjacentPanelId(
  layout: Layout | null | undefined,
  currentPanelId: string | undefined,
  direction: PaneFocusDirection,
): string | undefined {
  if (layout == null || currentPanelId === undefined) {
    return undefined;
  }
  const rects = paneRects(layout);
  const current = rects.get(currentPanelId);
  if (current === undefined) {
    return undefined;
  }
  const currentCenter = center(current);
  const ranked = [...rects.entries()]
    .filter(
      ([panelId, rect]) =>
        panelId !== currentPanelId && isInDirection(current, rect, direction),
    )
    .map(([panelId, rect], order) => {
      const candidateCenter = center(rect);
      return {
        panelId,
        order,
        primary: primaryGap(current, rect, direction),
        orthogonal: orthogonalGap(current, rect, direction),
        distance:
          Math.abs(candidateCenter.x - currentCenter.x) +
          Math.abs(candidateCenter.y - currentCenter.y),
      };
    })
    .sort(
      (a, b) =>
        a.primary - b.primary ||
        a.orthogonal - b.orthogonal ||
        a.distance - b.distance ||
        a.order - b.order,
    );
  return ranked[0]?.panelId;
}

function canvasPaneKey(pane: SessionCanvasPaneSnapshot): string {
  return pane.selected_panel_id ?? pane.panel_id;
}

function canvasPaneMatches(
  pane: SessionCanvasPaneSnapshot,
  panelId: string,
): boolean {
  return (
    pane.panel_id === panelId ||
    pane.selected_panel_id === panelId ||
    pane.panel_ids?.includes(panelId) === true
  );
}

/** Directional focus over persisted freeform canvas pane frames. */
export function adjacentCanvasPanelId(
  panes: readonly SessionCanvasPaneSnapshot[] | undefined,
  currentPanelId: string | undefined,
  direction: PaneFocusDirection,
): string | undefined {
  if (panes === undefined || currentPanelId === undefined) {
    return undefined;
  }
  const entries = panes
    .map((pane, order) => ({
      panelId: canvasPaneKey(pane),
      order,
      pane,
      rect: { x: pane.x, y: pane.y, w: pane.width, h: pane.height } satisfies Rect,
    }))
    .filter((entry) => entry.rect.w > 0 && entry.rect.h > 0);
  const currentEntry = entries.find((entry) =>
    canvasPaneMatches(entry.pane, currentPanelId),
  );
  if (currentEntry === undefined) {
    return undefined;
  }
  const current = currentEntry.rect;
  const currentCenter = center(current);
  const ranked = entries
    .filter(
      (entry) =>
        entry.order !== currentEntry.order &&
        isInDirection(current, entry.rect, direction),
    )
    .map((entry) => {
      const candidateCenter = center(entry.rect);
      return {
        panelId: entry.panelId,
        order: entry.order,
        primary: primaryGap(current, entry.rect, direction),
        orthogonal: orthogonalGap(current, entry.rect, direction),
        distance:
          Math.abs(candidateCenter.x - currentCenter.x) +
          Math.abs(candidateCenter.y - currentCenter.y),
      };
    })
    .sort(
      (a, b) =>
        a.primary - b.primary ||
        a.orthogonal - b.orthogonal ||
        a.distance - b.distance ||
        a.order - b.order,
    );
  return ranked[0]?.panelId;
}

/** The resolved focused panel id, subscribed to the singleton store. */
export function useFocusedPanelId(layout: Layout | null | undefined): string | undefined {
  // The third (server-snapshot) argument keeps SSR test renders working — the
  // palette host renders under `renderToStaticMarkup` in tests.
  const focused = useSyncExternalStore(
    focusedPaneStore.subscribe,
    focusedPaneStore.get,
    focusedPaneStore.get,
  );
  return resolveActivePanelId(focused, layout);
}
