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

/// Modifier state of a sidebar workspace click. `toggle` is Ctrl (or Meta) —
/// the Windows chord for canonical Cmd (`NSEvent.modifierFlags.contains(.command)`,
/// ContentView.swift:14257).
export interface WorkspaceClickModifiers {
  shift: boolean;
  toggle: boolean;
}

/// The multi-selection and anchor produced by a workspace click.
export interface WorkspaceClickResult {
  selectedWorkspaceIds: Set<string>;
  anchorIndex: number;
}

/// Multi-selection update for a workspace row click — a branch-for-branch port
/// of `updateSelection()` (ContentView.swift:14252-14319). Activation of the
/// clicked workspace is NOT decided here: canonical `selectTab` runs
/// unconditionally after the selection update (ContentView.swift:14320), so
/// the caller must activate regardless of modifiers.
export function selectionAfterWorkspaceClick(args: {
  /// Index of the clicked workspace in `liveWorkspaceIds`.
  clickedIndex: number;
  modifiers: WorkspaceClickModifiers;
  existingAnchorIndex: number | undefined;
  /// The current multi-selection.
  selectedWorkspaceIds: ReadonlySet<string>;
  /// The active workspace id (canonical `tabManager.selectedTabId`).
  focusedWorkspaceId: string | undefined;
  /// Live workspace ids in session order, index-aligned with click indices.
  liveWorkspaceIds: readonly string[];
  /// Collapsed-group members that are not their group's anchor — excluded
  /// from shift ranges (ContentView.swift:14284-14299).
  hiddenWorkspaceIds: ReadonlySet<string>;
}): WorkspaceClickResult {
  const {
    clickedIndex,
    modifiers,
    existingAnchorIndex,
    selectedWorkspaceIds,
    focusedWorkspaceId,
    liveWorkspaceIds,
    hiddenWorkspaceIds,
  } = args;
  const clickedId = liveWorkspaceIds[clickedIndex];
  // Anchor resolution precedes the branch (ContentView.swift:14267-14274);
  // shift with no resolvable anchor falls through to the cmd/plain branches.
  const resolvedShiftAnchorIndex = modifiers.shift
    ? shiftClickAnchorIndex(
        existingAnchorIndex,
        selectedWorkspaceIds,
        focusedWorkspaceId,
        liveWorkspaceIds,
      )
    : undefined;
  let next: Set<string>;
  if (modifiers.shift && resolvedShiftAnchorIndex !== undefined) {
    // Shift range = [min(anchor, clicked) ... max(anchor, clicked)] over the
    // live list, minus hidden collapsed-group members
    // (ContentView.swift:14276-14299).
    const lower = Math.min(resolvedShiftAnchorIndex, clickedIndex);
    const upper = Math.max(resolvedShiftAnchorIndex, clickedIndex);
    const rangeIds = liveWorkspaceIds
      .slice(lower, upper + 1)
      .filter((id) => !hiddenWorkspaceIds.has(id));
    if (modifiers.toggle) {
      // Shift+Cmd unions the range into the existing selection
      // (ContentView.swift:14301, `formUnion`).
      next = new Set(selectedWorkspaceIds);
      for (const id of rangeIds) {
        next.add(id);
      }
    } else {
      // Plain shift replaces the selection with the range
      // (ContentView.swift:14303).
      next = new Set(rangeIds);
    }
  } else if (modifiers.toggle) {
    // Cmd toggles the clicked id; removal may empty the set — canonical
    // allows that (ContentView.swift:14306-14311).
    next = new Set(selectedWorkspaceIds);
    if (clickedId !== undefined) {
      if (next.has(clickedId)) {
        next.delete(clickedId);
      } else {
        next.add(clickedId);
      }
    }
  } else {
    // Plain click collapses to the clicked workspace
    // (ContentView.swift:14313).
    next = clickedId !== undefined ? new Set([clickedId]) : new Set();
  }
  return {
    selectedWorkspaceIds: next,
    // ContentView.swift:14315 — shift preserves the resolved anchor, plain
    // and cmd clicks move it to the clicked row.
    anchorIndex: anchorIndexAfterWorkspaceClick(
      modifiers.shift,
      resolvedShiftAnchorIndex,
      clickedIndex,
    ),
  };
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
