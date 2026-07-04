// New-workspace sidebar placement — port of:
// - `Sources/WorkspacePlacement+Resolution.swift:13-59`
//   (`effectivePlacement` 17-29, `insertionIndex` 34-59 — pure arithmetic,
//   staged in Swift for extraction into CmuxWorkspaces),
// - the `WorkspacePlacement` enum,
//   `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/WorkspacePlacement.swift:4-6`,
// - the composing fold `TabManager.newTabInsertIndex`,
//   `Sources/TabManager.swift:1473-1506`.
//
// This is the SIDEBAR-level placement for `app.newWorkspacePlacement`. The
// group-level placement (`WorkspaceGroupNewPlacement` + `placeWithinGroup`) is
// a different feature and deliberately NOT part of this module.
//
// All indices are small integers; like the Swift Ints, callers are trusted to
// pass integers (no silent float truncation is performed).

/**
 * Where a newly-created workspace lands in the sidebar. Exact Swift raw
 * values (WorkspacePlacement.swift:4-6) — note camelCase `afterCurrent`.
 */
export type WorkspacePlacement = "top" | "end" | "afterCurrent";

/**
 * Strict exact-match parse of a persisted raw value, mirroring Swift's plain
 * `WorkspacePlacement(rawValue:)` decode (invalid values are logged-and-
 * skipped by the settings store; catalog default is `"afterCurrent"`).
 *
 * Deliberately NOT the tolerant multi-spelling parse — that belongs to the
 * separate group-placement setting (`WorkspaceGroupNewPlacement`).
 */
export function parseWorkspacePlacement(raw: string): WorkspacePlacement | null {
  switch (raw) {
    case "top":
    case "end":
    case "afterCurrent":
      return raw;
    default:
      return null;
  }
}

/**
 * The current selection + pinned-prefix shape of the tab list, as consumed by
 * `insertionIndex`. Mirrors the Swift parameter list
 * (WorkspacePlacement+Resolution.swift:34-38).
 */
export interface InsertionContext {
  selectedIndex: number | null;
  selectedIsPinned: boolean;
  pinnedCount: number;
  totalCount: number;
}

/**
 * The insertion index for a new workspace under `placement`. Mirrors
 * `WorkspacePlacement.insertionIndex` (WorkspacePlacement+Resolution.swift:34-59)
 * exactly: pure arithmetic, every input clamped into the valid range —
 * negative totals, `pinnedCount > totalCount`, and out-of-range
 * `selectedIndex` never throw.
 */
export function insertionIndex(
  placement: WorkspacePlacement,
  { selectedIndex, selectedIsPinned, pinnedCount, totalCount }: InsertionContext,
): number {
  const clampedTotalCount = Math.max(0, totalCount);
  const clampedPinnedCount = Math.max(0, Math.min(pinnedCount, clampedTotalCount));

  switch (placement) {
    case "top":
      // Keep pinned workspaces grouped at the top by inserting ahead of unpinned items.
      return clampedPinnedCount;
    case "end":
      return clampedTotalCount;
    case "afterCurrent": {
      if (selectedIndex === null || clampedTotalCount === 0) {
        return clampedTotalCount;
      }
      const clampedSelectedIndex = Math.max(0, Math.min(selectedIndex, clampedTotalCount - 1));
      if (selectedIsPinned) {
        return clampedPinnedCount;
      }
      return Math.min(clampedSelectedIndex + 1, clampedTotalCount);
    }
  }
}

/** One tab in the already-resolved current sidebar order. */
export interface PlacementTab {
  id: string;
  isPinned: boolean;
}

/**
 * Folds a workspace-creation snapshot into `insertionIndex`. Mirrors
 * `TabManager.newTabInsertIndex` (TabManager.swift:1489-1505).
 *
 * The caller supplies the current ordered tab list (Swift's
 * `orderedLiveWorkspaceCreationTabs(from:) ?? snapshot.tabs` live-refresh is
 * host-side and already resolved here).
 *
 * `pinnedCount` counts pinned tabs across the WHOLE list, not just the
 * prefix — a pinned tab out of prefix position still increments the count,
 * exactly like the Swift reduce.
 */
export function newTabInsertIndex(
  placement: WorkspacePlacement,
  tabs: readonly PlacementTab[],
  selectedTabId: string | null,
  selectedTabWasPinned: boolean,
): number {
  const pinnedCount = tabs.reduce((partial, tab) => (tab.isPinned ? partial + 1 : partial), 0);

  switch (placement) {
    case "top":
      return pinnedCount;
    case "end":
      return tabs.length;
    case "afterCurrent": {
      if (selectedTabId !== null) {
        const selectedIndex = tabs.findIndex((tab) => tab.id === selectedTabId);
        if (selectedIndex !== -1) {
          return insertionIndex("afterCurrent", {
            selectedIndex,
            selectedIsPinned: selectedTabWasPinned,
            pinnedCount,
            totalCount: tabs.length,
          });
        }
      }
      return selectedTabWasPinned ? pinnedCount : tabs.length;
    }
  }
}

/**
 * The placement to apply for a new workspace. Mirrors
 * `WorkspacePlacement.effectivePlacement`
 * (WorkspacePlacement+Resolution.swift:17-29): an explicit call-site override
 * wins, then iMessage mode pins `"top"`, then the stored
 * `app.newWorkspacePlacement` setting (catalog default `"afterCurrent"`,
 * supplied by the web settings layer).
 *
 * iMessage mode is a macOS feature; web callers pass `false`, but the
 * parameter is kept for byte-level rule parity.
 */
export function effectivePlacement(
  placementOverride: WorkspacePlacement | null,
  iMessageMode: boolean,
  stored: WorkspacePlacement,
): WorkspacePlacement {
  if (placementOverride !== null) {
    return placementOverride;
  }
  if (iMessageMode) {
    return "top";
  }
  return stored;
}
