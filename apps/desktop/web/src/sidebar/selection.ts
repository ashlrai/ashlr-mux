// Hand-port of `crates/cmux-workspaces/src/selection_sync.rs` (a port of
// `CmuxFoundation/Sidebar/SidebarWorkspaceSelectionSyncPolicy.swift`).
//
// Pure policy reconciling the sidebar's multi-workspace selection against the
// live workspace list, and computing shift-click anchor indices. Operates only
// on workspace UUID strings and indices; holds no state and touches no UI.
//
// DIVERGENCE: index parameters/returns use plain JS `number` to mirror Swift's
// signed `Int` / Rust `i64` and the `indices.contains` bounds check (which
// tolerates arbitrary/negative inputs). UUIDs are `string`; `HashSet<Uuid>`
// becomes `ReadonlySet<string>`.

function firstIndexOf(
  liveWorkspaceIds: readonly string[],
  target: string,
): number | undefined {
  const index = liveWorkspaceIds.indexOf(target);
  return index === -1 ? undefined : index;
}

function firstIndexWhereSelected(
  liveWorkspaceIds: readonly string[],
  selectedWorkspaceIds: ReadonlySet<string>,
): number | undefined {
  const index = liveWorkspaceIds.findIndex((id) =>
    selectedWorkspaceIds.has(id),
  );
  return index === -1 ? undefined : index;
}

function containsIndex(
  liveWorkspaceIds: readonly string[],
  index: number,
): boolean {
  return index >= 0 && index < liveWorkspaceIds.length;
}

/// Filters a previous selection down to workspaces that still exist, falling
/// back to the provided selected workspace when nothing survives.
export function reconciledSelection(
  previousSelectionIds: ReadonlySet<string>,
  liveWorkspaceIds: readonly string[],
  fallbackSelectedWorkspaceId: string | undefined,
): Set<string> {
  const liveIdSet = new Set(liveWorkspaceIds);
  const liveSelectionIds = new Set<string>();
  for (const id of previousSelectionIds) {
    if (liveIdSet.has(id)) {
      liveSelectionIds.add(id);
    }
  }
  if (liveSelectionIds.size > 0) {
    return liveSelectionIds;
  }
  if (
    fallbackSelectedWorkspaceId !== undefined &&
    liveIdSet.has(fallbackSelectedWorkspaceId)
  ) {
    return new Set([fallbackSelectedWorkspaceId]);
  }
  return new Set();
}

/// Index of the preferred (or first selected) workspace in the live list.
export function anchorIndex(
  preferredWorkspaceId: string | undefined,
  selectedWorkspaceIds: ReadonlySet<string>,
  liveWorkspaceIds: readonly string[],
): number | undefined {
  if (
    preferredWorkspaceId !== undefined &&
    selectedWorkspaceIds.has(preferredWorkspaceId)
  ) {
    const index = firstIndexOf(liveWorkspaceIds, preferredWorkspaceId);
    if (index !== undefined) {
      return index;
    }
  }
  return firstIndexWhereSelected(liveWorkspaceIds, selectedWorkspaceIds);
}

/// Workspace id at an existing anchor index, if the index is still valid.
export function anchorWorkspaceId(
  existingAnchorIndex: number | undefined,
  liveWorkspaceIds: readonly string[],
): string | undefined {
  if (existingAnchorIndex === undefined) {
    return undefined;
  }
  if (!containsIndex(liveWorkspaceIds, existingAnchorIndex)) {
    return undefined;
  }
  return liveWorkspaceIds[existingAnchorIndex];
}

/// Anchor index to use for a shift-click range, deriving one from the current
/// selection or focus when no anchor exists yet.
export function shiftClickAnchorIndex(
  existingAnchorIndex: number | undefined,
  selectedWorkspaceIds: ReadonlySet<string>,
  focusedWorkspaceId: string | undefined,
  liveWorkspaceIds: readonly string[],
): number | undefined {
  if (
    existingAnchorIndex !== undefined &&
    containsIndex(liveWorkspaceIds, existingAnchorIndex)
  ) {
    return existingAnchorIndex;
  }
  if (selectedWorkspaceIds.size === 1) {
    const selected = selectedWorkspaceIds.values().next().value;
    if (selected !== undefined) {
      const index = firstIndexOf(liveWorkspaceIds, selected);
      if (index !== undefined) {
        return index;
      }
    }
  }
  if (focusedWorkspaceId !== undefined) {
    return firstIndexOf(liveWorkspaceIds, focusedWorkspaceId);
  }
  return undefined;
}

/// Resulting anchor index after a workspace click (shift vs plain).
export function anchorIndexAfterWorkspaceClick(
  isShiftClick: boolean,
  resolvedShiftAnchorIndex: number | undefined,
  clickedIndex: number,
): number {
  if (isShiftClick) {
    return resolvedShiftAnchorIndex ?? clickedIndex;
  }
  return clickedIndex;
}

/// Anchor index to preserve after the workspace list is reordered.
export function anchorIndexAfterWorkspaceReorder(
  preferredAnchorWorkspaceId: string | undefined,
  selectedWorkspaceIds: ReadonlySet<string>,
  focusedWorkspaceId: string | undefined,
  liveWorkspaceIds: readonly string[],
): number | undefined {
  if (
    preferredAnchorWorkspaceId !== undefined &&
    selectedWorkspaceIds.has(preferredAnchorWorkspaceId)
  ) {
    const index = firstIndexOf(liveWorkspaceIds, preferredAnchorWorkspaceId);
    if (index !== undefined) {
      return index;
    }
  }
  return anchorIndex(
    focusedWorkspaceId,
    selectedWorkspaceIds,
    liveWorkspaceIds,
  );
}
