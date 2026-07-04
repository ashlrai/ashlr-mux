// Pure batch-reorder planning over a snapshot of the window's workspace
// (tab) order — port of
// `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Reorder/WorkspaceReorderPlanner.swift:12-67`
// (`batchReorderPlan` 19-46, `batchReorderFinalIds` 51-66) and its value
// types `WorkspaceOrderSnapshot.swift:5-16`, `WorkspaceReorderPlanItem.swift:5-19`,
// `WorkspaceBatchReorderError.swift:4-9`.
//
// Validates a requested id order and computes the final order under the
// pinned-ahead-of-unpinned invariant with stable ordering for unmentioned
// workspaces. Applying the plan stays with the owner.
//
// SANCTIONED DIVERGENCE (id representation): Swift UUIDs become opaque
// strings compared with exact string equality. Callers must pass ids in one
// canonical case — canonical LOWERCASE per the repo's cmux-diff UUID decision
// (commit b1fac5203). Swift UUID equality is on the 128-bit value, so
// mixed-case duplicates that Swift would catch are NOT caught by string
// equality; the canonical-lowercase contract mitigates this.

/** One workspace's position-relevant state. Mirrors `WorkspaceOrderSnapshot`. */
export interface WorkspaceOrderSnapshot {
  id: string;
  isPinned: boolean;
}

/** One requested workspace's move. Mirrors `WorkspaceReorderPlanItem`. */
export interface WorkspaceReorderPlanItem {
  workspaceId: string;
  fromIndex: number;
  toIndex: number;
}

/**
 * First validation failure for a batch reorder request. Mirrors
 * `WorkspaceBatchReorderError` (duplicateWorkspace / workspaceNotFound).
 */
export interface WorkspaceBatchReorderError {
  kind: "duplicateWorkspace" | "workspaceNotFound";
  workspaceId: string;
}

/** Result of `batchReorderPlan`, mirroring Swift's `Result`. */
export type WorkspaceBatchReorderResult =
  | { ok: true; plan: WorkspaceReorderPlanItem[] }
  | { ok: false; error: WorkspaceBatchReorderError };

/**
 * Computes the full final id order for the batch reorder. Mirrors
 * `batchReorderFinalIds` (WorkspaceReorderPlanner.swift:51-66): requested
 * pinned ids (request order), remaining pinned ids (current order),
 * requested unpinned ids (request order), remaining unpinned ids (current
 * order) — so a requested unpinned id can never land ahead of any pinned id.
 *
 * Callable WITHOUT validation, exactly like the Swift original: a requested
 * id absent from `current` fails both pinned filters (`undefined !== true`,
 * `undefined !== false`) and silently drops from the output.
 *
 * PRECONDITION: `current` must not contain duplicate ids — Swift builds
 * `snapshotsById` with `Dictionary(uniqueKeysWithValues:)`
 * (WorkspaceReorderPlanner.swift:56), which TRAPS on a duplicate id, so this
 * port throws an `Error` rather than silently taking last-wins (a plain
 * `new Map` keeps the last duplicate). Matches the same trap `batchReorderPlan`
 * replicates while building `currentIndexes`.
 */
export function batchReorderFinalIds(
  orderedWorkspaceIds: readonly string[],
  current: readonly WorkspaceOrderSnapshot[],
): string[] {
  const orderedSet = new Set(orderedWorkspaceIds);
  const snapshotsById = new Map<string, WorkspaceOrderSnapshot>();
  for (const snapshot of current) {
    if (snapshotsById.has(snapshot.id)) {
      // Mirrors the Swift Dictionary(uniqueKeysWithValues:) trap.
      throw new Error(`duplicate workspace id in current order: ${snapshot.id}`);
    }
    snapshotsById.set(snapshot.id, snapshot);
  }
  const orderedPinnedIds = orderedWorkspaceIds.filter(
    (id) => snapshotsById.get(id)?.isPinned === true,
  );
  const orderedUnpinnedIds = orderedWorkspaceIds.filter(
    (id) => snapshotsById.get(id)?.isPinned === false,
  );
  const remainingPinnedIds = current
    .map((snapshot) => snapshot.id)
    .filter((id) => !orderedSet.has(id) && snapshotsById.get(id)?.isPinned === true);
  const remainingUnpinnedIds = current
    .map((snapshot) => snapshot.id)
    .filter((id) => !orderedSet.has(id) && snapshotsById.get(id)?.isPinned === false);
  return [...orderedPinnedIds, ...remainingPinnedIds, ...orderedUnpinnedIds, ...remainingUnpinnedIds];
}

/**
 * Validates `orderedWorkspaceIds` against `current` and returns the
 * per-workspace move plan, or the first validation failure. Mirrors
 * `batchReorderPlan` (WorkspaceReorderPlanner.swift:19-46) —
 * VALIDATION ORDER MATTERS:
 *
 * 1. The whole duplicate scan runs BEFORE any unknown-id check (Swift
 *    oracle: `[known, known, unknown]` → duplicateWorkspace(known), NOT
 *    workspaceNotFound).
 * 2. Then the first requested id (in request order) missing from `current`
 *    → workspaceNotFound.
 *
 * Plan items exist only for REQUESTED ids, in request order. An empty
 * request succeeds with an empty plan.
 *
 * PRECONDITION: `current` must not contain duplicate ids — Swift builds its
 * index with `Dictionary(uniqueKeysWithValues:)`, which TRAPS on duplicates,
 * so this port throws an `Error` rather than silently taking last-wins.
 */
export function batchReorderPlan(
  orderedWorkspaceIds: readonly string[],
  current: readonly WorkspaceOrderSnapshot[],
): WorkspaceBatchReorderResult {
  const seen = new Set<string>();
  for (const workspaceId of orderedWorkspaceIds) {
    if (seen.has(workspaceId)) {
      return { ok: false, error: { kind: "duplicateWorkspace", workspaceId } };
    }
    seen.add(workspaceId);
  }

  const currentIndexes = new Map<string, number>();
  current.forEach((snapshot, index) => {
    if (currentIndexes.has(snapshot.id)) {
      // Mirrors the Swift Dictionary(uniqueKeysWithValues:) trap.
      throw new Error(`duplicate workspace id in current order: ${snapshot.id}`);
    }
    currentIndexes.set(snapshot.id, index);
  });
  for (const workspaceId of orderedWorkspaceIds) {
    if (!currentIndexes.has(workspaceId)) {
      return { ok: false, error: { kind: "workspaceNotFound", workspaceId } };
    }
  }

  const finalIds = batchReorderFinalIds(orderedWorkspaceIds, current);
  const finalIndexes = new Map(finalIds.map((id, index) => [id, index]));

  const plan = orderedWorkspaceIds.map((workspaceId) => ({
    workspaceId,
    // The ?? 0 fallbacks are unreachable post-validation but kept for
    // line-parity with the Swift `?? 0`s.
    fromIndex: currentIndexes.get(workspaceId) ?? 0,
    toIndex: finalIndexes.get(workspaceId) ?? 0,
  }));
  return { ok: true, plan };
}
