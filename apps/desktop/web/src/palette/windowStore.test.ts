import { describe, expect, test } from "bun:test";

import {
  CommandPaletteWindowStore,
  emptyDebugSnapshot,
  type CommandPaletteDebugSnapshot,
} from "./windowStore";

// Faithful ports of the Rust `window_store.rs` test suite (itself a port of
// `CommandPaletteWindowStoreTests.swift`), pinned to the Swift-derived
// expectations. Window ids are plain strings here.

const { REQUEST_GRACE_INTERVAL, PENDING_OPEN_MAX_AGE, ESCAPE_SUPPRESSION_INTERVAL } =
  CommandPaletteWindowStore;

describe("CommandPaletteWindowStore", () => {
  test("register seeds baseline", () => {
    const store = new CommandPaletteWindowStore();
    store.registerWindow("w");
    expect(store.isVisible("w")).toBe(false);
    expect(store.selectionIndex("w")).toBe(0);
    expect(store.snapshot("w").mode).toBe("commands");
    expect(store.snapshot("w").results).toHaveLength(0);
  });

  test("remove clears state", () => {
    const store = new CommandPaletteWindowStore();
    store.registerWindow("w");
    store.setVisible(true, "w");
    store.markOpenRequested("w", 100);
    store.beginEscapeSuppression("w", 100);
    store.setSelectionIndex(3, "w");
    store.removeWindow("w");
    expect(store.isVisible("w")).toBe(false);
    expect(store.isPendingOpenRaw("w")).toBe(false);
    expect(store.selectionIndex("w")).toBe(0);
    expect(store.firstVisibleWindowId()).toBeNull();
    expect(store.firstPendingOpenWindowId()).toBeNull();
  });

  test("pending-open is live within max age and pruned after", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 100);
    expect(store.isPendingOpen("w", 100 + PENDING_OPEN_MAX_AGE)).toBe(true);
    expect(store.isPendingOpen("w", 100 + PENDING_OPEN_MAX_AGE + 0.01)).toBe(false);
  });

  test("recentRequestAge returns age only within the grace interval", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 100);
    expect(store.recentRequestAge("w", 100 + REQUEST_GRACE_INTERVAL)).toBe(
      REQUEST_GRACE_INTERVAL,
    );
    expect(store.recentRequestAge("w", 100 + REQUEST_GRACE_INTERVAL + 0.01)).toBeNull();
  });

  test("setPendingOpenAge seam drives recentRequestAge", () => {
    const store = new CommandPaletteWindowStore();
    store.setPendingOpenAge("w", 200, 1.0);
    expect(store.recentRequestAge("w", 200)).toBe(1.0);
    store.setPendingOpenAge("w", 200, 6.25);
    expect(store.recentRequestAge("w", 200)).toBeNull();
  });

  test("escape suppression is consumed only within the suppression window", () => {
    const store = new CommandPaletteWindowStore();
    store.beginEscapeSuppression("w", 100);
    expect(
      store.shouldConsumeSuppressedEscape("w", 100 + ESCAPE_SUPPRESSION_INTERVAL),
    ).toBe(true);
    store.beginEscapeSuppression("w", 100);
    // Past the window: not consumed and cleaned up.
    expect(
      store.shouldConsumeSuppressedEscape("w", 100 + ESCAPE_SUPPRESSION_INTERVAL + 0.01),
    ).toBe(false);
    expect(store.shouldConsumeSuppressedEscape("w", 100)).toBe(false);
  });

  test("repeated false visibility retains an in-flight pending-open", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 100);
    const update = store.setVisible(false, "w");
    expect(update.wasVisible).toBe(false);
    expect(update.retainedPending).toBe(true);
    expect(store.isPendingOpenRaw("w")).toBe(true);
  });

  test("opening then closing clears pending-open", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 100);
    const open = store.setVisible(true, "w");
    expect(open.wasVisible).toBe(false);
    expect(store.isPendingOpenRaw("w")).toBe(false);
    const close = store.setVisible(false, "w");
    expect(close.wasVisible).toBe(true);
    expect(store.isVisible("w")).toBe(false);
  });

  test("selection index is clamped to zero", () => {
    const store = new CommandPaletteWindowStore();
    store.setSelectionIndex(-5, "w");
    expect(store.selectionIndex("w")).toBe(0);
    store.setSelectionIndex(7, "w");
    expect(store.selectionIndex("w")).toBe(7);
  });

  test("prune reports the stale outcome", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("stale", 0);
    const pruned = store.pruneExpiredPendingOpenStates(PENDING_OPEN_MAX_AGE + 1);
    expect(pruned).toHaveLength(1);
    expect(pruned[0]).toEqual({ kind: "stale", windowId: "stale", age: PENDING_OPEN_MAX_AGE + 1 });
  });

  test("stale prune removes both the flag and the timestamp", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 0);
    const pruned = store.pruneExpiredPendingOpenStates(9);
    expect(pruned).toHaveLength(1);
    expect(store.isPendingOpenRaw("w")).toBe(false);
    expect(store.recentRequestAge("w", 9)).toBeNull();
  });

  test("pending-open survives at exactly the max-age boundary", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 100);
    const pruned = store.pruneExpiredPendingOpenStates(100 + PENDING_OPEN_MAX_AGE);
    expect(pruned).toHaveLength(0);
    expect(store.isPendingOpenRaw("w")).toBe(true);
  });

  test("recentRequestAge is null after a stale prune", () => {
    const store = new CommandPaletteWindowStore();
    store.markOpenRequested("w", 0);
    expect(store.recentRequestAge("w", 100)).toBeNull();
    expect(store.isPendingOpenRaw("w")).toBe(false);
  });

  test("setVisible(true) when already visible clears pending", () => {
    const store = new CommandPaletteWindowStore();
    store.setVisible(true, "w");
    store.markOpenRequested("w", 100);
    const update = store.setVisible(true, "w");
    expect(update.wasVisible).toBe(true);
    expect(update.retainedPending).toBe(false);
    expect(store.isPendingOpenRaw("w")).toBe(false);
  });

  test("shouldConsumeSuppressedEscape is false for an unknown window", () => {
    const store = new CommandPaletteWindowStore();
    expect(store.shouldConsumeSuppressedEscape("unknown", 0)).toBe(false);
  });

  test("clearAllEscapeSuppression clears every window", () => {
    const store = new CommandPaletteWindowStore();
    store.beginEscapeSuppression("a", 0);
    store.beginEscapeSuppression("b", 0);
    store.clearAllEscapeSuppression();
    expect(store.shouldConsumeSuppressedEscape("a", 0)).toBe(false);
    expect(store.shouldConsumeSuppressedEscape("b", 0)).toBe(false);
  });

  test("snapshot round-trips through the store unchanged", () => {
    const store = new CommandPaletteWindowStore();
    const snapshot: CommandPaletteDebugSnapshot = {
      query: "query",
      mode: "rename_input",
      results: [
        {
          commandId: "command.rename",
          title: "Rename",
          shortcutHint: "R",
          trailingLabel: null,
          score: 42,
        },
      ],
    };
    store.setSnapshot(snapshot, "w");
    expect(store.snapshot("w")).toEqual(snapshot);
  });

  test("snapshot defaults to empty for an unregistered window", () => {
    const store = new CommandPaletteWindowStore();
    expect(store.snapshot("nope")).toEqual(emptyDebugSnapshot());
  });
});
