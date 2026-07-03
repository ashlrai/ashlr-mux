import type { Layout, Pane, SplitPath } from "./splitLayout";
import { clampDivider, isPane } from "./splitLayout";
import { normalizeSurfaceKind, type SurfaceKind } from "./surfaceUrl";

/**
 * Flat-layer geometry for the snapshot-driven workspace.
 *
 * The recursive `SplitTree` renderer nests panes inside the tree, so a split
 * moves a pane's DOM position and React remounts it — which would kill the
 * pane's live shell. The workspace instead renders every terminal ONCE in a
 * flat, absolutely-positioned portal layer keyed by a stable `panel_id`. These
 * pure functions turn a layout tree into the rectangles that layer needs:
 *
 *  - {@link paneRects}: `panel_id → {x,y,w,h}` (percentages of the container).
 *  - {@link dividerHandles}: one entry per split node — its path, orientation,
 *    parent rect and ratio — so the overlay can draw + drag each divider.
 *
 * Rects are pure percentages (panes meet exactly at divider centerlines);
 * the caller insets each pane by the divider thickness at render time, where
 * pixel dimensions are known. Divider math (`clampDivider`) is shared with the
 * `SplitTree` path so both renderers stay byte-identical to the macOS bonsplit
 * geometry.
 */

/** A rectangle in percentage-of-container units (0–100 on each axis). */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A draggable divider: which split it belongs to and where it sits. */
export interface DividerHandle {
  /** Path from the root split to this split node (for `session_set_divider`). */
  path: SplitPath;
  orientation: "horizontal" | "vertical";
  /** The rect of the split node's whole area (both children + the divider). */
  parent: Rect;
  /** The (clamped) divider ratio of this split. */
  ratio: number;
}

/** The full container. */
const FULL: Rect = { x: 0, y: 0, w: 100, h: 100 };

/** The representative panel id of a pane (its selected panel, else the first). */
function representativeId(pane: Pane): string | undefined {
  return pane.selected_panel_id ?? pane.panel_ids[0];
}

/**
 * The two child rects of a split occupying `rect`. A `horizontal` split places
 * children side-by-side (first = left `ratio` share); a `vertical` split stacks
 * them (first = top `ratio` share). Children meet at the divider centerline.
 */
function childRects(rect: Rect, orientation: "horizontal" | "vertical", ratio: number): [Rect, Rect] {
  if (orientation === "horizontal") {
    const firstW = rect.w * ratio;
    return [
      { x: rect.x, y: rect.y, w: firstW, h: rect.h },
      { x: rect.x + firstW, y: rect.y, w: rect.w - firstW, h: rect.h },
    ];
  }
  const firstH = rect.h * ratio;
  return [
    { x: rect.x, y: rect.y, w: rect.w, h: firstH },
    { x: rect.x, y: rect.y + firstH, w: rect.w, h: rect.h - firstH },
  ];
}

/** Map each leaf pane's representative `panel_id` to its rect (percentages). */
export function paneRects(layout: Layout, rect: Rect = FULL): Map<string, Rect> {
  const out = new Map<string, Rect>();
  walk(layout, rect);
  return out;

  function walk(node: Layout, r: Rect): void {
    if (isPane(node)) {
      const id = representativeId(node.pane);
      if (id !== undefined) {
        out.set(id, r);
      }
      return;
    }
    const ratio = clampDivider(node.split.divider_position);
    const [first, second] = childRects(r, node.split.orientation, ratio);
    walk(node.split.first, first);
    walk(node.split.second, second);
  }
}

/**
 * Map each leaf pane's representative `panel_id` to its normalized
 * {@link SurfaceKind} — the pane's raw `surface_kind` (an arbitrary
 * `Option<String>` in the Rust model) run through {@link normalizeSurfaceKind},
 * so an absent/unknown value resolves to `"terminal"`. Keyed exactly like
 * {@link paneRects}, so the flat portal can branch each pane's surface over a
 * closed set of kinds.
 */
export function surfaceKinds(layout: Layout): Map<string, SurfaceKind> {
  const out = new Map<string, SurfaceKind>();
  walk(layout);
  return out;

  function walk(node: Layout): void {
    if (isPane(node)) {
      const id = representativeId(node.pane);
      if (id !== undefined) {
        out.set(id, normalizeSurfaceKind(node.pane.surface_kind));
      }
      return;
    }
    walk(node.split.first);
    walk(node.split.second);
  }
}

/** One {@link DividerHandle} per split node, in depth-first order. */
export function dividerHandles(layout: Layout, rect: Rect = FULL): DividerHandle[] {
  const out: DividerHandle[] = [];
  walk(layout, rect, []);
  return out;

  function walk(node: Layout, r: Rect, path: ReadonlyArray<"first" | "second">): void {
    if (isPane(node)) {
      return;
    }
    const ratio = clampDivider(node.split.divider_position);
    out.push({ path, orientation: node.split.orientation, parent: r, ratio });
    const [first, second] = childRects(r, node.split.orientation, ratio);
    walk(node.split.first, first, [...path, "first"]);
    walk(node.split.second, second, [...path, "second"]);
  }
}
