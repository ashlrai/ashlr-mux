import type {
  SessionPaneLayoutSnapshot,
  SessionSplitLayoutSnapshot,
  SessionWorkspaceLayoutSnapshot,
} from "@cmux/core-types";

/**
 * Pure split-tree helpers over the generated `@cmux/core-types` session model.
 *
 * The layout is a binary tree: each node is either a `pane` (leaf) or a `split`
 * (two children + orientation + one `divider_position` ratio). These helpers
 * are the Windows/React port of the macOS bonsplit divider math
 * (`CmuxPanes/Geometry/ExternalTreeNode+SplitGeometry.swift`) — same clamp
 * range, same leaf-weighted equalize — kept pure so they unit-test headlessly
 * and the React renderer stays a thin shell over them.
 */

export type Layout = SessionWorkspaceLayoutSnapshot;
export type Split = SessionSplitLayoutSnapshot;
export type Pane = SessionPaneLayoutSnapshot;

/** A split node addressed by the sequence of child steps from the root. */
export type SplitPath = ReadonlyArray<"first" | "second">;

/**
 * Divider ratios are clamped to [0.1, 0.9] — byte-for-byte the macOS bonsplit
 * bound (`ExternalTreeNode+SplitGeometry.swift` resize clamp). Keeps a pane from
 * collapsing to zero and matches drag limits on macOS.
 */
export const MIN_DIVIDER = 0.1;
export const MAX_DIVIDER = 0.9;

export function isPane(layout: Layout): layout is { type: "pane"; pane: Pane } {
  return layout.type === "pane";
}

export function isSplit(layout: Layout): layout is { type: "split"; split: Split } {
  return layout.type === "split";
}

/** Clamp a divider ratio into the legal range; NaN falls back to centered. */
export function clampDivider(position: number): number {
  if (Number.isNaN(position)) {
    return 0.5;
  }
  return Math.min(MAX_DIVIDER, Math.max(MIN_DIVIDER, position));
}

/**
 * The active panel id of the first (top-left-most) leaf pane: its selected
 * panel, else its first panel. `undefined` only for a malformed empty pane.
 * Interim split-target until focused-pane tracking (C4) lands — exact for the
 * common single-pane workspace, where the first leaf IS the focused pane.
 */
export function firstActivePanelId(layout: Layout): string | undefined {
  if (isPane(layout)) {
    return layout.pane.selected_panel_id ?? layout.pane.panel_ids[0];
  }
  return (
    firstActivePanelId(layout.split.first) ??
    firstActivePanelId(layout.split.second)
  );
}

/** Number of leaf panes in a subtree. */
export function countLeaves(layout: Layout): number {
  return isPane(layout)
    ? 1
    : countLeaves(layout.split.first) + countLeaves(layout.split.second);
}

/**
 * Orientation-aware span count: how many same-orientation panes a subtree
 * contributes along `orientation`. A leaf is 1 span; a split whose orientation
 * matches sums its children's spans; a CROSS-orientation nested split counts as
 * a single span (not its leaf total). Mirrors macOS `spanCount(along:)` in
 * `ExternalTreeNode+SplitGeometry.swift`, the source of truth for equalize.
 */
export function spanCount(layout: Layout, orientation: "horizontal" | "vertical"): number {
  if (isPane(layout)) {
    return 1;
  }
  if (layout.split.orientation !== orientation) {
    return 1;
  }
  return (
    spanCount(layout.split.first, orientation) + spanCount(layout.split.second, orientation)
  );
}

/**
 * The divider ratio after a drag of `deltaPixels` along a `axisPixels`-long
 * axis, clamped. Mirrors macOS `resizeDividerAdjustment`
 * (`delta = amountPixels / axisPixels`, applied to `dividerPosition`).
 */
export function resizeDivider(current: number, deltaPixels: number, axisPixels: number): number {
  if (axisPixels <= 0) {
    return clampDivider(current);
  }
  return clampDivider(current + deltaPixels / axisPixels);
}

/**
 * The equalized divider ratio = the first subtree's share of same-orientation
 * spans. Mirrors macOS `equalizeDividerPlan` (`firstSpanCount / totalSpanCount`
 * with an ORIENTATION-AWARE `spanCount(along:)`), so a cross-orientation nested
 * subtree counts as a single span, not its leaf total.
 *
 * DELIBERATE CORRECTION: this previously weighted by `countLeaves` (every leaf,
 * ignoring orientation), which diverged from macOS — a `horizontal` split over
 * a `vertical` [a,b] subtree and a `c` leaf equalized to 2/3 instead of the
 * canonical 1/2. Now weighted by `spanCount` along the split's own orientation.
 */
export function equalizeDivider(split: Split): number {
  const first = spanCount(split.first, split.orientation);
  const total = first + spanCount(split.second, split.orientation);
  // Equalize does NOT clamp (parity with macOS `appendEqualizeAdjustments`,
  // which emits `CGFloat(firstSpanCount) / CGFloat(totalSpanCount)` raw, and
  // the sibling web `equalizeDividerPlan`): the span ratio of a valid tree is
  // always in (0, 1). The total===0 guard only defends against an empty tree.
  return total === 0 ? 0.5 : first / total;
}

/**
 * Immutably set the `divider_position` of the split reached by `path`
 * (empty path = the root split). Returns the root unchanged if the path runs
 * off a leaf, so callers never have to pre-validate. The input tree is never
 * mutated.
 */
export function setDividerAtPath(root: Layout, path: SplitPath, position: number): Layout {
  if (!isSplit(root)) {
    return root;
  }
  if (path.length === 0) {
    return { type: "split", split: { ...root.split, divider_position: clampDivider(position) } };
  }
  const [head, ...rest] = path;
  const updatedChild = setDividerAtPath(root.split[head], rest, position);
  return { type: "split", split: { ...root.split, [head]: updatedChild } };
}
