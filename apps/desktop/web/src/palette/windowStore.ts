// Port of `crates/cmux-command-palette/src/window_store.rs` (itself a port of
// `State/CommandPaletteWindowStore.swift`) — the pure per-window command-palette
// state machine: visibility, pending-open (with expiry), escape suppression,
// selection index, and the debug snapshot.
//
// Timing is `now` seconds passed in by the caller (Swift's
// `ProcessInfo.systemUptime`) so the logic stays pure and headless-testable.
//
// DIVERGENCE (parity-preserving): the Swift/Rust dictionaries are unordered, so
// `first*WindowId` is documented as arbitrary and only relied on for the
// empty / single-window cases. A JS `Map` iterates in insertion order, which
// satisfies that contract deterministically (single-window MVP today).

/** A single rendered result row captured for command-palette debug inspection. */
export interface CommandPaletteDebugResultRow {
  commandId: string;
  title: string;
  shortcutHint: string | null;
  trailingLabel: string | null;
  score: number;
}

/** A point-in-time snapshot of the command-palette contents for a window. */
export interface CommandPaletteDebugSnapshot {
  query: string;
  mode: string;
  results: CommandPaletteDebugResultRow[];
}

/** The empty snapshot (Swift `static let empty`): `""` / `"commands"` / `[]`. */
export function emptyDebugSnapshot(): CommandPaletteDebugSnapshot {
  return { query: "", mode: "commands", results: [] };
}

/** The outcome of pruning one stale pending-open entry, for debug logging. */
export type PrunedPendingOpen =
  | { kind: "missingTimestamp"; windowId: string }
  | { kind: "stale"; windowId: string; age: number };

/** The result of a visibility update: the prior value + whether a pending-open
 *  request was retained across a `false`→`false` update. */
export interface VisibilityUpdate {
  wasVisible: boolean;
  retainedPending: boolean;
}

/**
 * Owns the per-window command-palette state for every main window, keyed by a
 * window id string. Mirrors `CommandPaletteWindowStore`.
 */
export class CommandPaletteWindowStore {
  /** Grace window during which a recent palette request is still "fresh". */
  static readonly REQUEST_GRACE_INTERVAL = 1.25;
  /** Maximum age before a pending-open request is pruned as stale. */
  static readonly PENDING_OPEN_MAX_AGE = 8.0;
  /** Window during which a suppressed escape key-up is consumed. */
  static readonly ESCAPE_SUPPRESSION_INTERVAL = 0.35;

  private readonly visibilityByWindowId = new Map<string, boolean>();
  private readonly pendingOpenByWindowId = new Map<string, boolean>();
  private readonly recentRequestAtByWindowId = new Map<string, number>();
  private readonly escapeSuppressionByWindowId = new Set<string>();
  private readonly escapeSuppressionStartedAtByWindowId = new Map<string, number>();
  private readonly selectionByWindowId = new Map<string, number>();
  private readonly snapshotByWindowId = new Map<string, CommandPaletteDebugSnapshot>();

  // --- Registration / teardown ---------------------------------------------

  /** Seeds the baseline palette state for a newly registered window. */
  registerWindow(windowId: string): void {
    this.visibilityByWindowId.set(windowId, false);
    this.selectionByWindowId.set(windowId, 0);
    this.snapshotByWindowId.set(windowId, emptyDebugSnapshot());
  }

  /** Removes every piece of palette state for a window being torn down. */
  removeWindow(windowId: string): void {
    this.visibilityByWindowId.delete(windowId);
    this.pendingOpenByWindowId.delete(windowId);
    this.recentRequestAtByWindowId.delete(windowId);
    this.escapeSuppressionByWindowId.delete(windowId);
    this.escapeSuppressionStartedAtByWindowId.delete(windowId);
    this.selectionByWindowId.delete(windowId);
    this.snapshotByWindowId.delete(windowId);
  }

  // --- Pending-open ---------------------------------------------------------

  /** Marks a window as having requested a palette open at `now`. */
  markOpenRequested(windowId: string, now: number): void {
    this.pendingOpenByWindowId.set(windowId, true);
    this.recentRequestAtByWindowId.set(windowId, now);
  }

  /** Clears the pending-open request for a window. */
  clearPendingOpen(windowId: string): void {
    this.pendingOpenByWindowId.delete(windowId);
    this.recentRequestAtByWindowId.delete(windowId);
  }

  /** Prunes pending-open entries older than {@link PENDING_OPEN_MAX_AGE},
   *  returning what was pruned so callers can match the debug-log behavior. */
  pruneExpiredPendingOpenStates(now: number): PrunedPendingOpen[] {
    const pruned: PrunedPendingOpen[] = [];
    // Snapshot the keys before mutation (Swift iterates `Array(keys)`).
    for (const windowId of [...this.pendingOpenByWindowId.keys()]) {
      if (this.pendingOpenByWindowId.get(windowId) !== true) {
        continue;
      }
      const requestedAt = this.recentRequestAtByWindowId.get(windowId);
      if (requestedAt === undefined) {
        this.pendingOpenByWindowId.delete(windowId);
        pruned.push({ kind: "missingTimestamp", windowId });
        continue;
      }
      const age = now - requestedAt;
      if (age <= CommandPaletteWindowStore.PENDING_OPEN_MAX_AGE) {
        continue;
      }
      this.pendingOpenByWindowId.delete(windowId);
      this.recentRequestAtByWindowId.delete(windowId);
      pruned.push({ kind: "stale", windowId, age });
    }
    return pruned;
  }

  /** Whether a window has a live pending-open request, after pruning stale ones. */
  isPendingOpen(windowId: string, now: number): boolean {
    this.pruneExpiredPendingOpenStates(now);
    return this.pendingOpenByWindowId.get(windowId) === true;
  }

  /** Raw pending-open flag without pruning. */
  isPendingOpenRaw(windowId: string): boolean {
    return this.pendingOpenByWindowId.get(windowId) === true;
  }

  /** The age of a recent, still-fresh palette request, or `null` when none applies. */
  recentRequestAge(windowId: string, now: number): number | null {
    this.pruneExpiredPendingOpenStates(now);
    if (this.pendingOpenByWindowId.get(windowId) !== true) {
      this.recentRequestAtByWindowId.delete(windowId);
      return null;
    }
    const startedAt = this.recentRequestAtByWindowId.get(windowId);
    if (startedAt === undefined) {
      this.pendingOpenByWindowId.delete(windowId);
      return null;
    }
    const age = now - startedAt;
    return age <= CommandPaletteWindowStore.REQUEST_GRACE_INTERVAL ? age : null;
  }

  /** The first window id with a live pending-open request, if any. */
  firstPendingOpenWindowId(): string | null {
    for (const [id, open] of this.pendingOpenByWindowId) {
      if (open) {
        return id;
      }
    }
    return null;
  }

  /** Test seam: forces a window's pending-open request to a given age. */
  setPendingOpenAge(windowId: string, now: number, age: number): void {
    this.pendingOpenByWindowId.set(windowId, true);
    this.recentRequestAtByWindowId.set(windowId, now - Math.max(age, 0));
  }

  // --- Escape suppression ---------------------------------------------------

  /** Begins escape suppression for a window at `now`. */
  beginEscapeSuppression(windowId: string, now: number): void {
    this.escapeSuppressionByWindowId.add(windowId);
    this.escapeSuppressionStartedAtByWindowId.set(windowId, now);
  }

  /** Ends escape suppression for a window. */
  endEscapeSuppression(windowId: string): void {
    this.escapeSuppressionByWindowId.delete(windowId);
    this.escapeSuppressionStartedAtByWindowId.delete(windowId);
  }

  /** Whether a suppressed escape should be consumed at `now`. When suppression
   *  has expired the entry is cleaned up (fallback for a lost key-up). */
  shouldConsumeSuppressedEscape(windowId: string, now: number): boolean {
    if (!this.escapeSuppressionByWindowId.has(windowId)) {
      return false;
    }
    const startedAt = this.escapeSuppressionStartedAtByWindowId.get(windowId) ?? 0;
    if (now - startedAt <= CommandPaletteWindowStore.ESCAPE_SUPPRESSION_INTERVAL) {
      return true;
    }
    this.endEscapeSuppression(windowId);
    return false;
  }

  /** Clears escape suppression for every window (fallback when none resolves). */
  clearAllEscapeSuppression(): void {
    this.escapeSuppressionByWindowId.clear();
    this.escapeSuppressionStartedAtByWindowId.clear();
  }

  // --- Visibility -----------------------------------------------------------

  /** Updates a window's visibility, clearing pending-open on open/close.
   *  Repeated `false` updates are ignored so a stale sync can't erase an
   *  in-flight open request. */
  setVisible(visible: boolean, windowId: string): VisibilityUpdate {
    // Swift `updateValue` returns the OLD value (nil → false).
    const wasVisible = this.visibilityByWindowId.get(windowId) ?? false;
    this.visibilityByWindowId.set(windowId, visible);
    if (visible || wasVisible) {
      this.pendingOpenByWindowId.delete(windowId);
      this.recentRequestAtByWindowId.delete(windowId);
    }
    const retainedPending =
      !visible && !wasVisible && this.pendingOpenByWindowId.get(windowId) === true;
    return { wasVisible, retainedPending };
  }

  /** Whether the palette is marked visible for a window. */
  isVisible(windowId: string): boolean {
    return this.visibilityByWindowId.get(windowId) ?? false;
  }

  /** The first window id with the palette currently visible, if any. */
  firstVisibleWindowId(): string | null {
    for (const [id, visible] of this.visibilityByWindowId) {
      if (visible) {
        return id;
      }
    }
    return null;
  }

  // --- Selection ------------------------------------------------------------

  /** Sets the clamped (>= 0) selection index for a window. */
  setSelectionIndex(index: number, windowId: string): void {
    this.selectionByWindowId.set(windowId, Math.max(index, 0));
  }

  /** The selection index for a window, defaulting to zero. */
  selectionIndex(windowId: string): number {
    return this.selectionByWindowId.get(windowId) ?? 0;
  }

  // --- Snapshot -------------------------------------------------------------

  /** Stores the debug snapshot for a window. */
  setSnapshot(snapshot: CommandPaletteDebugSnapshot, windowId: string): void {
    this.snapshotByWindowId.set(windowId, snapshot);
  }

  /** The debug snapshot for a window, defaulting to empty. */
  snapshot(windowId: string): CommandPaletteDebugSnapshot {
    return this.snapshotByWindowId.get(windowId) ?? emptyDebugSnapshot();
  }
}
