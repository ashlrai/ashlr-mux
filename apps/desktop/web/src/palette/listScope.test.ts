import { describe, expect, test } from "bun:test";

import {
  listScope,
  queryForMatching,
  shouldPromoteOverlay,
  shouldResetVisibleResults,
  trimWhitespaceAndNewlines,
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

  // Parity with Swift `CharacterSet.whitespacesAndNewlines`, which diverges
  // from JS `String.prototype.trim()` on exactly two code points.
  test("trims U+0085 (NEL) like Swift, unlike JS .trim()", () => {
    // Swift's `.newlines` includes U+0085; JS `.trim()` does not. A lone NEL
    // query must trim to empty so the `isEmpty`-gated switcher behavior
    // (`commandPaletteSwitcherIncludesSurfaceEntries`) matches the host.
    expect(queryForMatching("")).toBe("");
    expect(queryForMatching(">")).toBe("");
    // NEL surrounding real content is stripped from both ends.
    expect(queryForMatching("main")).toBe("main");
  });

  test("preserves leading U+FEFF (BOM) like Swift, unlike JS .trim()", () => {
    // U+FEFF is Unicode category Cf (not Zs), so Swift keeps it; JS `.trim()`
    // strips it, which would wrongly yield "project" / "rename".
    expect(queryForMatching("﻿project")).toBe("﻿project");
    expect(queryForMatching(">﻿rename")).toBe("﻿rename");
  });
});

describe("trimWhitespaceAndNewlines", () => {
  test("trims ASCII whitespace and newlines from both ends", () => {
    expect(trimWhitespaceAndNewlines("  hi  ")).toBe("hi");
    expect(trimWhitespaceAndNewlines("\t\n\r\fhi\f\r\n\t")).toBe("hi");
    expect(trimWhitespaceAndNewlines("a b")).toBe("a b");
  });

  test("matches Swift's set, not JS .trim(): trims U+0085, keeps U+FEFF", () => {
    expect(trimWhitespaceAndNewlines("hi")).toBe("hi");
    expect(trimWhitespaceAndNewlines("﻿hi﻿")).toBe("﻿hi﻿");
  });

  test("trims the Unicode Zs separators Swift's `.whitespaces` covers", () => {
    // U+00A0 NBSP, U+2003 EM SPACE, U+3000 IDEOGRAPHIC SPACE, U+2028 LS.
    expect(trimWhitespaceAndNewlines("  　 hi 　")).toBe("hi");
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
