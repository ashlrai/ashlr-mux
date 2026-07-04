// Oracle tests for placement.ts, pinned to Swift semantics.
//
// The first block ports the Swift oracle rows verbatim from
// `cmuxTests/WorkspaceUnitTests.swift:2892-2936`; the remaining tables are
// hand-derived from `Sources/WorkspacePlacement+Resolution.swift:34-59` and
// `Sources/TabManager.swift:1489-1505`.

import { describe, expect, test } from "bun:test";

import {
  effectivePlacement,
  insertionIndex,
  newTabInsertIndex,
  parseWorkspacePlacement,
  type PlacementTab,
} from "./placement";

describe("insertionIndex — Swift oracle (WorkspaceUnitTests.swift:2892-2936)", () => {
  test("top inserts before unpinned (after the pinned prefix)", () => {
    expect(
      insertionIndex("top", {
        selectedIndex: 4,
        selectedIsPinned: false,
        pinnedCount: 2,
        totalCount: 7,
      }),
    ).toBe(2);
  });

  test("afterCurrent handles pinned and unpinned selection", () => {
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: 3,
        selectedIsPinned: false,
        pinnedCount: 2,
        totalCount: 6,
      }),
    ).toBe(4);
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: 0,
        selectedIsPinned: true,
        pinnedCount: 2,
        totalCount: 6,
      }),
    ).toBe(2);
  });

  test("end and no-selection append", () => {
    expect(
      insertionIndex("end", {
        selectedIndex: 1,
        selectedIsPinned: false,
        pinnedCount: 1,
        totalCount: 5,
      }),
    ).toBe(5);
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: null,
        selectedIsPinned: false,
        pinnedCount: 0,
        totalCount: 5,
      }),
    ).toBe(5);
  });
});

describe("insertionIndex — clamping (Swift lines 40-41, 50-57)", () => {
  test("negative totalCount clamps to 0 for top and end", () => {
    const ctx = { selectedIndex: null, selectedIsPinned: false, pinnedCount: 2, totalCount: -3 };
    expect(insertionIndex("top", ctx)).toBe(0);
    expect(insertionIndex("end", ctx)).toBe(0);
  });

  test("pinnedCount above totalCount clamps to totalCount", () => {
    expect(
      insertionIndex("top", {
        selectedIndex: null,
        selectedIsPinned: false,
        pinnedCount: 9,
        totalCount: 4,
      }),
    ).toBe(4);
  });

  test("negative pinnedCount clamps to 0", () => {
    expect(
      insertionIndex("top", {
        selectedIndex: null,
        selectedIsPinned: false,
        pinnedCount: -2,
        totalCount: 4,
      }),
    ).toBe(0);
  });

  test("out-of-range selectedIndex clamps to last index (then +1 caps at total)", () => {
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: 99,
        selectedIsPinned: false,
        pinnedCount: 1,
        totalCount: 5,
      }),
    ).toBe(5);
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: -2,
        selectedIsPinned: false,
        pinnedCount: 1,
        totalCount: 5,
      }),
    ).toBe(1);
  });

  test("afterCurrent with zero total returns 0 even with a selection", () => {
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: 0,
        selectedIsPinned: true,
        pinnedCount: 0,
        totalCount: 0,
      }),
    ).toBe(0);
    expect(
      insertionIndex("afterCurrent", {
        selectedIndex: 2,
        selectedIsPinned: false,
        pinnedCount: 0,
        totalCount: 0,
      }),
    ).toBe(0);
  });
});

describe("newTabInsertIndex — TabManager.swift:1489-1505", () => {
  const tabs: PlacementTab[] = [
    { id: "p1", isPinned: true },
    { id: "p2", isPinned: true },
    { id: "u1", isPinned: false },
    { id: "u2", isPinned: false },
    { id: "u3", isPinned: false },
    { id: "u4", isPinned: false },
  ];

  test("top → pinnedCount; end → length", () => {
    expect(newTabInsertIndex("top", tabs, null, false)).toBe(2);
    expect(newTabInsertIndex("end", tabs, null, false)).toBe(6);
  });

  test("afterCurrent with found selected id delegates to insertionIndex", () => {
    // Reproduces the Swift oracle row afterCurrent(3,false,2,6) → 4.
    expect(newTabInsertIndex("afterCurrent", tabs, "u2", false)).toBe(4);
    // Pinned selection lands right after the pinned prefix.
    expect(newTabInsertIndex("afterCurrent", tabs, "p1", true)).toBe(2);
  });

  test("afterCurrent with missing/absent selected id falls back on the pinned flag", () => {
    expect(newTabInsertIndex("afterCurrent", tabs, "ghost", true)).toBe(2); // pinnedCount
    expect(newTabInsertIndex("afterCurrent", tabs, "ghost", false)).toBe(6); // length
    expect(newTabInsertIndex("afterCurrent", tabs, null, true)).toBe(2);
    expect(newTabInsertIndex("afterCurrent", tabs, null, false)).toBe(6);
  });

  test("pinned tab OUT of prefix position still counts toward pinnedCount", () => {
    // Swift reduces over the WHOLE list, not the prefix.
    const scattered: PlacementTab[] = [
      { id: "u1", isPinned: false },
      { id: "p1", isPinned: true },
      { id: "u2", isPinned: false },
      { id: "p2", isPinned: true },
    ];
    expect(newTabInsertIndex("top", scattered, null, false)).toBe(2);
    expect(newTabInsertIndex("afterCurrent", scattered, "ghost", true)).toBe(2);
  });

  test("empty tab list", () => {
    expect(newTabInsertIndex("top", [], null, false)).toBe(0);
    expect(newTabInsertIndex("end", [], null, false)).toBe(0);
    expect(newTabInsertIndex("afterCurrent", [], null, false)).toBe(0);
  });
});

describe("effectivePlacement — WorkspacePlacement+Resolution.swift:17-29", () => {
  test("explicit override wins over everything", () => {
    expect(effectivePlacement("end", true, "afterCurrent")).toBe("end");
    expect(effectivePlacement("top", false, "end")).toBe("top");
  });

  test("iMessage mode pins top over the stored setting", () => {
    expect(effectivePlacement(null, true, "afterCurrent")).toBe("top");
    expect(effectivePlacement(null, true, "end")).toBe("top");
  });

  test("otherwise the stored setting applies", () => {
    expect(effectivePlacement(null, false, "afterCurrent")).toBe("afterCurrent");
    expect(effectivePlacement(null, false, "end")).toBe("end");
  });
});

describe("parseWorkspacePlacement — strict rawValue parse", () => {
  test("accepts exactly the three raw values", () => {
    expect(parseWorkspacePlacement("top")).toBe("top");
    expect(parseWorkspacePlacement("end")).toBe("end");
    expect(parseWorkspacePlacement("afterCurrent")).toBe("afterCurrent");
  });

  test("rejects other spellings, case variants, and padding", () => {
    expect(parseWorkspacePlacement("aftercurrent")).toBeNull();
    expect(parseWorkspacePlacement("after-current")).toBeNull();
    expect(parseWorkspacePlacement(" top")).toBeNull();
    expect(parseWorkspacePlacement("END")).toBeNull();
    expect(parseWorkspacePlacement("")).toBeNull();
    expect(parseWorkspacePlacement("nope")).toBeNull();
  });
});
