import { describe, expect, test } from "bun:test";

import {
  createFocusedPaneStore,
  panelIdsInLayout,
  resolveActivePanelId,
} from "./focusedPane";
import type { Layout } from "./splitLayout";

function pane(panelIds: string[], selected?: string): Layout {
  return { type: "pane", pane: { panel_ids: panelIds, selected_panel_id: selected } };
}

function split(
  orientation: "horizontal" | "vertical",
  divider: number,
  first: Layout,
  second: Layout,
): Layout {
  return { type: "split", split: { orientation, divider_position: divider, first, second } };
}

describe("createFocusedPaneStore", () => {
  test("starts with no focused pane", () => {
    expect(createFocusedPaneStore().get()).toBeUndefined();
  });

  test("focus records the panel id and get returns it", () => {
    const store = createFocusedPaneStore();
    store.focus("a");
    expect(store.get()).toBe("a");
    store.focus("b");
    expect(store.get()).toBe("b");
  });

  test("subscribers are notified on change", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    expect(notified).toBe(1);
    store.focus("b");
    expect(notified).toBe(2);
  });

  test("re-focusing the same id does NOT notify (event-spam no-op)", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    store.focus("a");
    store.focus("a");
    expect(notified).toBe(1);
    expect(store.get()).toBe("a");
  });

  test("unsubscribe stops notifications", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    const unsubscribe = store.subscribe(() => {
      notified += 1;
    });
    store.focus("a");
    unsubscribe();
    store.focus("b");
    expect(notified).toBe(1);
    expect(store.get()).toBe("b");
  });

  test("clear resets to no focus and notifies; clearing empty is a no-op", () => {
    const store = createFocusedPaneStore();
    let notified = 0;
    store.subscribe(() => {
      notified += 1;
    });
    store.clear();
    expect(notified).toBe(0);
    store.focus("a");
    store.clear();
    expect(store.get()).toBeUndefined();
    expect(notified).toBe(2);
  });
});

describe("panelIdsInLayout", () => {
  test("collects every panel id of every leaf, not just the selected ones", () => {
    const layout = split(
      "horizontal",
      0.5,
      pane(["a1", "a2"], "a1"),
      split("vertical", 0.5, pane(["b"]), pane(["c"])),
    );
    expect([...panelIdsInLayout(layout)].sort()).toEqual(["a1", "a2", "b", "c"]);
  });
});

describe("resolveActivePanelId", () => {
  const twoPanes = split("horizontal", 0.5, pane(["a"]), pane(["b"]));

  test("no layout resolves to undefined regardless of focus", () => {
    expect(resolveActivePanelId("a", undefined)).toBeUndefined();
    expect(resolveActivePanelId("a", null)).toBeUndefined();
    expect(resolveActivePanelId(undefined, undefined)).toBeUndefined();
  });

  test("a focused id living in the first leaf is returned verbatim", () => {
    expect(resolveActivePanelId("a", twoPanes)).toBe("a");
  });

  test("a focused id living in a NON-first leaf is returned verbatim", () => {
    expect(resolveActivePanelId("b", twoPanes)).toBe("b");
  });

  test("a focused id that is a non-selected member of a pane's panel_ids still resolves", () => {
    const layout = split("horizontal", 0.5, pane(["a1", "a2"], "a1"), pane(["b"]));
    expect(resolveActivePanelId("a2", layout)).toBe("a2");
  });

  test("a stale focused id falls back to the first active panel", () => {
    expect(resolveActivePanelId("gone", twoPanes)).toBe("a");
  });

  test("no focus falls back to the first active panel", () => {
    expect(resolveActivePanelId(undefined, twoPanes)).toBe("a");
  });

  test("a malformed empty pane resolves to undefined", () => {
    expect(resolveActivePanelId(undefined, pane([]))).toBeUndefined();
  });

  test("workspace switch: the raw focus survives and re-resolves per layout", () => {
    // Focus "b" in workspace 1, switch to workspace 2 (no "b") — falls back to
    // workspace 2's first leaf; switch back — the remembered focus resolves
    // again. Panel ids are globally unique, so the stale id cannot collide.
    const store = createFocusedPaneStore();
    const workspace2 = split("vertical", 0.5, pane(["x"]), pane(["y"]));
    store.focus("b");
    expect(resolveActivePanelId(store.get(), workspace2)).toBe("x");
    expect(resolveActivePanelId(store.get(), twoPanes)).toBe("b");
  });
});
