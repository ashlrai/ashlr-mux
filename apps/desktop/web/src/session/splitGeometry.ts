import type { Pane, SplitPath } from "./splitLayout";
import { clampDivider, isPane, type Layout, spanCount } from "./splitLayout";

/**
 * Pure split-tree planning over the generated `@cmux/core-types` session model:
 * equalize and keyboard-resize divider math, ported one-for-one from the macOS
 * `ExternalTreeNode+SplitGeometry.swift` (`equalizeDividerPlan` /
 * `resizeDividerAdjustment`). Plans are derived from the layout snapshot only,
 * so they unit-test headlessly; the React renderer applies them via
 * `setDividerAtPath` (see `splitLayout.ts`).
 *
 * DIVERGENCE — split addressing: macOS addresses splits by `UUID` and reports a
 * `hadInvalidSplitIds` flag when a node's id fails to parse. The web layout tree
 * carries no split ids; every split is addressed by its {@link SplitPath} from
 * the root (as `session_set_divider` / `paneRects` already do). A path is always
 * valid, so the Swift `hadInvalidSplitIds` branch and its guarded id-parse are
 * dropped here — every matching split yields an adjustment.
 */

/** A pixel-sized container (the DOM element the layout renders into). */
export interface ContainerSize {
  width: number;
  height: number;
}

/**
 * A planned divider mutation: set the split reached by `path` to `position`.
 * Web addresses splits by PATH, not the Swift `splitId: UUID` (see DIVERGENCE
 * in the module header).
 */
export interface SplitDividerAdjustment {
  path: SplitPath;
  position: number;
}

/** The result of an equalize pass: one adjustment per matching split. */
export interface SplitEqualizePlan {
  adjustments: SplitDividerAdjustment[];
  /** Whether any split matched the orientation filter (mirrors macOS). */
  foundSplit: boolean;
}

// ---------------------------------------------------------------------------
// Direction value maps (ported from CmuxPanes/Values/{Resize,Split}Direction).
// ---------------------------------------------------------------------------

/** Keyboard resize direction (moves the pane's controlling divider). */
export type ResizeDirection = "left" | "right" | "up" | "down";

interface ResizeDirectionSpec {
  /** Orientation string of the split whose divider this resize moves. */
  splitOrientation: "horizontal" | "vertical";
  /** A split controls the target's right/bottom edge when the target is the
   *  first child (right/down), and the left/top edge when it is the second. */
  requiresPaneInFirstChild: boolean;
  /** +1 moves the divider toward the second child (right/down), -1 the first. */
  dividerDeltaSign: 1 | -1;
}

/** Value map for {@link ResizeDirection} — mirrors macOS `ResizeDirection`. */
export const RESIZE_DIRECTION: Record<ResizeDirection, ResizeDirectionSpec> = {
  left: { splitOrientation: "horizontal", requiresPaneInFirstChild: false, dividerDeltaSign: -1 },
  right: { splitOrientation: "horizontal", requiresPaneInFirstChild: true, dividerDeltaSign: 1 },
  up: { splitOrientation: "vertical", requiresPaneInFirstChild: false, dividerDeltaSign: -1 },
  down: { splitOrientation: "vertical", requiresPaneInFirstChild: true, dividerDeltaSign: 1 },
};

/** Split-insertion direction (which side a new pane is inserted on). */
export type SplitDirection = "left" | "right" | "up" | "down";

interface SplitDirectionSpec {
  /** Whether the split divides space horizontally (left/right). */
  isHorizontal: boolean;
  /** The orientation of the new split. */
  orientation: "horizontal" | "vertical";
  /** Insert the new pane on the "first" side (left/top) when true. */
  insertFirst: boolean;
}

/** Value map for {@link SplitDirection} — mirrors macOS `SplitDirection`. */
export const SPLIT_DIRECTION: Record<SplitDirection, SplitDirectionSpec> = {
  left: { isHorizontal: true, orientation: "horizontal", insertFirst: true },
  right: { isHorizontal: true, orientation: "horizontal", insertFirst: false },
  up: { isHorizontal: false, orientation: "vertical", insertFirst: true },
  down: { isHorizontal: false, orientation: "vertical", insertFirst: false },
};

// ---------------------------------------------------------------------------
// Equalize planning.
// ---------------------------------------------------------------------------

/** The representative panel id of a pane (its selected panel, else the first). */
function representativeId(pane: Pane): string | undefined {
  return pane.selected_panel_id ?? pane.panel_ids[0];
}

/**
 * Plan an equalize pass: every split matching `orientationFilter` (every split
 * when omitted) gets its divider set so each same-orientation span receives
 * equal space, using the ORIENTATION-AWARE {@link spanCount} (a cross-orientation
 * nested subtree counts as 1 span, not its leaf total).
 *
 * Adjustments are emitted post-order (children before their parent), matching
 * the legacy macOS mutation order in `appendEqualizeAdjustments`.
 */
export function equalizeDividerPlan(
  root: Layout,
  orientationFilter?: "horizontal" | "vertical",
): SplitEqualizePlan {
  const adjustments: SplitDividerAdjustment[] = [];
  let foundSplit = false;

  const walk = (node: Layout, path: SplitPath): void => {
    if (isPane(node)) {
      return;
    }
    const { first, second, orientation } = node.split;
    walk(first, [...path, "first"]);
    walk(second, [...path, "second"]);

    if (orientationFilter === undefined || orientation === orientationFilter) {
      foundSplit = true;
      const firstSpan = spanCount(first, orientation);
      const totalSpan = firstSpan + spanCount(second, orientation);
      // Equalize does NOT clamp (parity with macOS): the span ratio of a valid
      // tree is always in (0, 1).
      const position = totalSpan === 0 ? 0.5 : firstSpan / totalSpan;
      adjustments.push({ path, position });
    }
  };

  walk(root, []);
  return { adjustments, foundSplit };
}

// ---------------------------------------------------------------------------
// Resize planning.
// ---------------------------------------------------------------------------

interface ResizeSplitCandidate {
  path: SplitPath;
  orientation: "horizontal" | "vertical";
  paneInFirstChild: boolean;
  dividerPosition: number;
  axisPixels: number;
}

/** The two child rects (percentage-of-container) of a split, mirroring paneRects. */
function childSpan(
  span: { w: number; h: number },
  orientation: "horizontal" | "vertical",
  ratio: number,
): [{ w: number; h: number }, { w: number; h: number }] {
  if (orientation === "horizontal") {
    const firstW = span.w * ratio;
    return [
      { w: firstW, h: span.h },
      { w: span.w - firstW, h: span.h },
    ];
  }
  const firstH = span.h * ratio;
  return [
    { w: span.w, h: firstH },
    { w: span.w, h: span.h - firstH },
  ];
}

/**
 * Plan a keyboard resize of `targetPanelId`'s controlling divider: collect the
 * splits enclosing the target (innermost first, the legacy candidate order),
 * keep those matching the direction's orientation, pick the first whose child
 * side matches, and convert `amountPixels` into a divider delta along that
 * split's axis, clamped to [0.1, 0.9]. Returns `null` when the pane is absent or
 * no enclosing split matches.
 *
 * The target pane is the one whose representative panel (`selected_panel_id ??
 * panel_ids[0]`) equals `targetPanelId`. The split's axis length in pixels is
 * derived from its percentage-of-container span times `container`, mirroring
 * `Workspace.tsx` `beginDividerDrag` (`axisPixels = container.(w|h) * span/100`).
 */
export function resizeDividerAdjustment(
  root: Layout,
  targetPanelId: string,
  direction: ResizeDirection,
  amountPixels: number,
  container: ContainerSize,
): SplitDividerAdjustment | null {
  const spec = RESIZE_DIRECTION[direction];
  const candidates: ResizeSplitCandidate[] = [];

  const collect = (
    node: Layout,
    span: { w: number; h: number },
    path: SplitPath,
  ): boolean => {
    if (isPane(node)) {
      return representativeId(node.pane) === targetPanelId;
    }
    const { first, second, orientation, divider_position } = node.split;
    const ratio = clampDivider(divider_position);
    const [firstSpan, secondSpan] = childSpan(span, orientation, ratio);
    const inFirst = collect(first, firstSpan, [...path, "first"]);
    const inSecond = collect(second, secondSpan, [...path, "second"]);
    const containsTarget = inFirst || inSecond;

    if (containsTarget) {
      const axisPixels =
        orientation === "horizontal"
          ? (container.width * span.w) / 100
          : (container.height * span.h) / 100;
      candidates.push({
        path,
        orientation,
        paneInFirstChild: inFirst,
        // Raw divider position (unclamped), mirroring macOS which reads
        // split.dividerPosition and clamps only the final requested value.
        dividerPosition: divider_position,
        axisPixels: Math.max(axisPixels, 1),
      });
    }
    return containsTarget;
  };

  const containsTarget = collect(root, { w: 100, h: 100 }, []);
  if (!containsTarget) {
    return null;
  }

  const orientationMatches = candidates.filter(
    (c) => c.orientation === spec.splitOrientation,
  );
  if (orientationMatches.length === 0) {
    return null;
  }

  const candidate = orientationMatches.find(
    (c) => c.paneInFirstChild === spec.requiresPaneInFirstChild,
  );
  if (candidate === undefined) {
    return null;
  }

  const delta = amountPixels / candidate.axisPixels;
  const requested = candidate.dividerPosition + spec.dividerDeltaSign * delta;
  // Same [0.1, 0.9] bound as macOS and clampDivider.
  const position = clampDivider(requested);
  return { path: candidate.path, position };
}
