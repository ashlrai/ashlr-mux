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

/** Number of leaf panes in a subtree. */
export function countLeaves(layout: Layout): number {
  return isPane(layout)
    ? 1
    : countLeaves(layout.split.first) + countLeaves(layout.split.second);
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
 * The equalized divider ratio = the first subtree's share of leaf panes.
 * Mirrors macOS `equalizeDividerPlan` (`firstSpanCount / totalSpanCount`), so
 * an equalize weights by pane count, not raw depth.
 */
export function equalizeDivider(split: Split): number {
  const first = countLeaves(split.first);
  const total = first + countLeaves(split.second);
  return total === 0 ? 0.5 : clampDivider(first / total);
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
