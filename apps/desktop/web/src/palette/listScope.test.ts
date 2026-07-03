import { describe, expect, test } from "bun:test";

import {
  listScope,
  queryForMatching,
  shouldPromoteOverlay,
  shouldResetVisibleResults,
} from "./listScope";

describe("listScope", () => {
  test("a `>`-prefixed query selects the commands scope", () => {
    expect(listScope(">")).toBe("commands");
    expect(listScope(">rename")).toBe("commands");
    expect(listScope(">  spaced")).toBe("commands");
  });

  test("any other query selects the switcher scope", () => {
    expect(listScope("")).toBe("switcher");
    expect(listScope("rename")).toBe("switcher");
    expect(listScope(" >not-at-start")).toBe("switcher");
  });
});

describe("queryForMatching", () => {
  test("commands scope drops the `>` and trims", () => {
    expect(queryForMatching(">rename")).toBe("rename");
    expect(queryForMatching(">  rename tab  ")).toBe("rename tab");
    expect(queryForMatching(">")).toBe("");
  });

  test("switcher scope trims but keeps the whole query", () => {
    expect(queryForMatching("  project  ")).toBe("project");
    expect(queryForMatching("main")).toBe("main");
  });
});

describe("shouldPromoteOverlay", () => {
  // Mirrors Rust `promotes_only_on_hidden_to_visible_transition`.
  test("promotes only on the hidden→visible transition", () => {
    expect(shouldPromoteOverlay(false, true)).toBe(true);
    expect(shouldPromoteOverlay(true, true)).toBe(false);
    expect(shouldPromoteOverlay(false, false)).toBe(false);
    expect(shouldPromoteOverlay(true, false)).toBe(false);
  });
});

describe("shouldResetVisibleResults", () => {
  test("resets only when results are shown and the scope flips", () => {
    // switcher → commands, results visible.
    expect(shouldResetVisibleResults("main", ">", true)).toBe(true);
    // commands → switcher, results visible.
    expect(shouldResetVisibleResults(">rename", "rename", true)).toBe(true);
  });

  test("does not reset when the scope is unchanged", () => {
    expect(shouldResetVisibleResults("main", "project", true)).toBe(false);
    expect(shouldResetVisibleResults(">a", ">b", true)).toBe(false);
  });

  test("does not reset when no results are visible", () => {
    expect(shouldResetVisibleResults("main", ">", false)).toBe(false);
  });
});
