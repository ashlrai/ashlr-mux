// Oracle tests for settingsSearch.ts.
//
// The scorer/alias/builder cases are a 1:1 port of the crate oracle block in
// `crates/cmux-settings-search/src/lib.rs:33-555` (the byte-faithful oracle for
// the scorer + aliases). The producer-level cases (beyond the crate, which does
// not port `entries(matching:)`) pin the corpus + producer against
// `Sources/SettingsNavigation.swift:304-590`.

import { describe, expect, test } from "bun:test";

import {
  aliases,
  allEntries,
  containsAtWordBoundary,
  editDistance,
  isLightTypo,
  isSearchStopWord,
  isSubsequence,
  makeEntryForTest,
  matchScore,
  matchScoreToken,
  normalized,
  normalizedQueryTokens,
  normalizedTokens,
  sectionAliases,
  sectionEntries,
  sectionId,
  settingAliases,
  settingEntries,
  settingEntry,
  settingId,
  settingsEntriesMatching,
  targetFromRawValue,
  type SettingsNavigationTarget,
  type SettingsSearchEntry,
  SETTINGS_NAVIGATION_TARGETS,
} from "./settingsSearch";

function wordsAndSet(text: string): {
  words: string[];
  set: Set<string>;
} {
  const words = normalizedTokens(text);
  return { words, set: new Set(words) };
}

// ----- normalized -----

describe("normalized", () => {
  test("lowercases ascii", () => {
    expect(normalized("MixedCASE")).toBe("mixedcase");
    expect(normalized("App.Language")).toBe("app.language");
  });

  test("strips precomposed diacritics", () => {
    expect(normalized("Café")).toBe("cafe");
    expect(normalized("naïve")).toBe("naive");
    expect(normalized("Ñoño")).toBe("nono");
  });

  test("strips combining marks", () => {
    // "e" + U+0301 COMBINING ACUTE ACCENT folds to "e".
    expect(normalized("é")).toBe("e");
  });

  test("preserves non-latin letters", () => {
    expect(normalized("日本語")).toBe("日本語");
  });
});

// ----- tokenizer -----

describe("normalizedTokens", () => {
  test("splits on dot, underscore, hyphen", () => {
    expect(normalizedTokens("app.language")).toEqual(["app", "language"]);
    expect(normalizedTokens("CMUX_PORT")).toEqual(["cmux", "port"]);
    expect(normalizedTokens("sidebar-font-size")).toEqual([
      "sidebar",
      "font",
      "size",
    ]);
  });

  test("keeps ascii symbol characters + and =", () => {
    expect(normalizedTokens("Show Branch + Directory")).toEqual([
      "show",
      "branch",
      "+",
      "directory",
    ]);
    expect(normalizedTokens("a=b")).toEqual(["a=b"]);
  });

  test("collapses empty runs", () => {
    expect(normalizedTokens("  leading  and   trailing  ")).toEqual([
      "leading",
      "and",
      "trailing",
    ]);
  });

  test("refolds diacritics", () => {
    expect(normalizedTokens("Café Münchén")).toEqual(["cafe", "munchen"]);
  });
});

// ----- stop words -----

describe("stop words", () => {
  test("is_search_stop_word true for setting(s)/preference(s), false else", () => {
    expect(isSearchStopWord("setting")).toBe(true);
    expect(isSearchStopWord("settings")).toBe(true);
    expect(isSearchStopWord("preference")).toBe(true);
    expect(isSearchStopWord("preferences")).toBe(true);
    expect(isSearchStopWord("appearance")).toBe(false);
  });

  test("normalizedQueryTokens drops them, normalizedTokens keeps them", () => {
    expect(
      normalizedQueryTokens("settings appearance preferences theme"),
    ).toEqual(["appearance", "theme"]);
    expect(normalizedTokens("settings appearance preferences theme")).toEqual([
      "settings",
      "appearance",
      "preferences",
      "theme",
    ]);
  });
});

// ----- inner matchScoreToken: one per branch -----

describe("matchScoreToken", () => {
  test("exact word scores zero", () => {
    const { words, set } = wordsAndSet("theme light");
    expect(matchScoreToken("theme", "theme light", words, set)).toBe(0);
  });

  test("prefix scores ten", () => {
    const { words, set } = wordsAndSet("theme");
    expect(matchScoreToken("the", "theme", words, set)).toBe(10);
  });

  test("word boundary scores twenty", () => {
    const { words, set } = wordsAndSet("a+b");
    expect(words).toEqual(["a+b"]);
    expect(matchScoreToken("b", "a+b", words, set)).toBe(20);
  });

  test("mid-word contains scores thirty", () => {
    const { words, set } = wordsAndSet("appearance");
    expect(matchScoreToken("ppe", "appearance", words, set)).toBe(30);
  });

  test("light typo scores fifty", () => {
    const { words, set } = wordsAndSet("system");
    expect(matchScoreToken("sysfem", "system", words, set)).toBe(50);
  });

  test("subsequence of word scores sixty", () => {
    const { words, set } = wordsAndSet("system");
    expect(matchScoreToken("sym", "system", words, set)).toBe(60);
  });

  test("subsequence of text scores eighty", () => {
    const { words, set } = wordsAndSet("app language");
    expect(matchScoreToken("apang", "app language", words, set)).toBe(80);
  });

  test("no match returns null", () => {
    const { words, set } = wordsAndSet("system");
    expect(matchScoreToken("xyz", "system", words, set)).toBeNull();
  });
});

// ----- containsAtWordBoundary -----

describe("containsAtWordBoundary", () => {
  test("empty needle is true", () => {
    expect(containsAtWordBoundary("", "anything")).toBe(true);
  });
  test("at start is true", () => {
    expect(containsAtWordBoundary("app", "appearance")).toBe(true);
  });
  test("after space is true", () => {
    expect(containsAtWordBoundary("theme", "app theme x")).toBe(true);
  });
  test("after symbol is true", () => {
    expect(containsAtWordBoundary("b", "a+b")).toBe(true);
  });
  test("mid-word is false", () => {
    expect(containsAtWordBoundary("pear", "appearance")).toBe(false);
    expect(containsAtWordBoundary("a", "cat")).toBe(false);
  });
});

// ----- isSubsequence -----

describe("isSubsequence", () => {
  test("cases", () => {
    expect(isSubsequence("", "x")).toBe(true);
    expect(isSubsequence("ace", "abcde")).toBe(true);
    expect(isSubsequence("aec", "abcde")).toBe(false);
    expect(isSubsequence("abcdef", "abc")).toBe(false);
  });
});

// ----- isLightTypo -----

describe("isLightTypo", () => {
  test("one insertion", () => {
    expect(isLightTypo("color", "colour")).toBe(true);
  });
  test("requires min length four", () => {
    expect(isLightTypo("cat", "cats")).toBe(false);
  });
  test("rejects two substitutions when allowed one", () => {
    expect(isLightTypo("abcd", "abyz")).toBe(false);
  });
  test("allows two when both at least six", () => {
    expect(isLightTypo("sysfem", "system")).toBe(true);
  });
  test("length delta gate", () => {
    expect(isLightTypo("abcdef", "abcdefxyz")).toBe(false);
  });
});

// ----- editDistance -----

describe("editDistance", () => {
  test("classic value", () => {
    expect(editDistance("kitten", "sitting", 10)).toBe(3);
  });
  test("length delta early exit", () => {
    const result = editDistance("ab", "abcdef", 1);
    expect(result).toBe(2);
    expect(result).toBeGreaterThan(1);
  });
  test("row minimum early exit", () => {
    const result = editDistance("abcd", "wxyz", 1);
    expect(result).toBe(2);
    expect(result).toBeGreaterThan(1);
  });
  test("exact within bound", () => {
    expect(editDistance("color", "colour", 1)).toBe(1);
  });
});

// ----- outer matchScore with bonuses -----

function appearanceEntry(): SettingsSearchEntry {
  return makeEntryForTest(
    "setting:app:appearance",
    "setting",
    "app",
    "Appearance",
    "App",
    "gearshape",
    "theme light dark system",
  );
}

describe("matchScore", () => {
  test("word boundary and contains bonuses", () => {
    const entry = appearanceEntry();
    // normalized_search_text = "appearance app theme light dark system".
    // token "theme" -> 0; then -500 (boundary in text) and -400 (contains).
    expect(matchScore(entry, "theme", ["theme"])).toBe(-900);
  });

  test("exact title stacks all title bonuses", () => {
    const entry = appearanceEntry();
    expect(matchScore(entry, "appearance", ["appearance"])).toBe(-4000);
  });

  test("section bonus adds twenty-five", () => {
    const entry = makeEntryForTest(
      "section:app",
      "section",
      "app",
      "App",
      null,
      "gearshape",
      "settings general",
    );
    // normalized_search_text = "app  settings general" (doubled space from the
    // null subtitle). query "app": -1000/-800/-700/-600/-500/-400 then +25.
    expect(matchScore(entry, "app", ["app"])).toBe(-3975);
  });

  test("returns null when a token is absent", () => {
    const entry = appearanceEntry();
    expect(matchScore(entry, "theme zzzz", ["theme", "zzzz"])).toBeNull();
  });
});

// ----- ranking over several entries -----

test("ranking orders by ascending score then offset", () => {
  const exactTitle = makeEntryForTest(
    "setting:browser:theme",
    "setting",
    "browser",
    "Theme",
    "Browser",
    "globe",
    "browser theme color scheme",
  );
  const keywordOnly = makeEntryForTest(
    "setting:app:appearance",
    "setting",
    "app",
    "Appearance",
    "App",
    "gearshape",
    "theme light dark system",
  );
  const subsequenceOnly = makeEntryForTest(
    "setting:app:telemetry",
    "setting",
    "app",
    "Telemetry",
    "App",
    "gearshape",
    // Contains "t..h..e..m..e" only as a spanning subsequence, no "theme".
    "the metrics enable",
  );

  const query = "theme";
  const tokens = normalizedQueryTokens(query);
  const normalizedQuery = normalized(query).trim();

  const entries = [exactTitle, keywordOnly, subsequenceOnly];
  const ranked = entries
    .map((entry, offset) => ({
      offset,
      id: entry.id,
      score: matchScore(entry, normalizedQuery, tokens),
    }))
    .filter((r): r is { offset: number; id: string; score: number } =>
      r.score !== null,
    );
  ranked.sort((a, b) =>
    a.score !== b.score ? a.score - b.score : a.offset - b.offset,
  );

  expect(ranked.map((r) => r.id)).toEqual([
    "setting:browser:theme",
    "setting:app:appearance",
    "setting:app:telemetry",
  ]);
  expect(ranked[0].score).toBeLessThan(ranked[1].score);
  expect(ranked[1].score).toBeLessThan(ranked[2].score);
});

// ----- alias table + builders -----

describe("aliases", () => {
  test("section aliases are verbatim", () => {
    expect(sectionAliases("terminal")).toBe(
      "shell scrollback scrollbar scroll bar ghostty tty pty",
    );
    expect(sectionAliases("reset")).toBe(
      "factory defaults restore clear preferences",
    );
  });

  test("setting aliases lookup hits and misses", () => {
    expect(settingAliases("account:account")).toBe(
      "auth authentication login logout signin sign-in signout sign-out email user profile stack team",
    );
    expect(settingAliases("reset:reset-all")).toBe(
      "factory reset restore defaults clear preferences",
    );
    expect(settingAliases("app:does-not-exist")).toBeNull();
  });

  test("missing key is empty string", () => {
    expect(aliases("app", "no-such-suffix")).toBe("");
  });

  test("keyboardShortcuts:shortcuts appends empty action list (trailing space)", () => {
    const expected = `${settingAliases("keyboardShortcuts:shortcuts")} `;
    expect(aliases("keyboardShortcuts", "shortcuts")).toBe(expected);
  });

  test("non-shortcut key has no trailing space", () => {
    expect(aliases("browser", "theme")).toBe(
      "browser.theme web page theme color scheme light dark system",
    );
  });
});

// ----- setting builder + id helpers -----

describe("builders", () => {
  test("id helpers format", () => {
    expect(sectionId("settingsJSON")).toBe("section:settingsJSON");
    expect(settingId("app", "appearance")).toBe("setting:app:appearance");
  });

  test("setting builder composes search text and matches", () => {
    const entry = settingEntry(
      "app",
      "appearance",
      "Appearance",
      "theme light dark system",
    );
    expect(entry.id).toBe("setting:app:appearance");
    expect(entry.kind).toBe("setting");
    expect(entry.subtitle).toBe("App");
    expect(entry.symbolName).toBe("gearshape");

    // The composed searchText folds in the alias table value, so an alias-only
    // keyword ("scheme") still matches.
    expect(entry.normalizedSearchWordSet.has("scheme")).toBe(true);
    expect(entry.normalizedSearchWordSet.has("appearance")).toBe(true);
    expect(entry.normalizedSearchWordSet.has("app")).toBe(true);
  });

  test("target round-trips raw value", () => {
    for (const target of SETTINGS_NAVIGATION_TARGETS) {
      expect(targetFromRawValue(target)).toBe(target);
    }
    expect(targetFromRawValue("nope")).toBeNull();
    const textBox: SettingsNavigationTarget = "textBox";
    expect(textBox).toBe("textBox");
    const settingsJson: SettingsNavigationTarget = "settingsJSON";
    expect(settingsJson).toBe("settingsJSON");
  });
});

// ----- producer-level (beyond the crate) -----

describe("settingsEntriesMatching", () => {
  test("empty query returns exactly the 16 section entries in allCases order", () => {
    const result = settingsEntriesMatching("");
    expect(result).toHaveLength(16);
    expect(result.map((e) => e.target)).toEqual([
      ...SETTINGS_NAVIGATION_TARGETS,
    ]);
    expect(result.every((e) => e.kind === "section")).toBe(true);
  });

  test("stop-words-only query returns the 16 section entries", () => {
    const result = settingsEntriesMatching("settings preferences");
    expect(result).toHaveLength(16);
    expect(result.every((e) => e.kind === "section")).toBe(true);
  });

  test("exact-title query ranks the App Appearance setting first", () => {
    const result = settingsEntriesMatching("appearance");
    expect(result[0]?.id).toBe("setting:app:appearance");
  });

  test("keyword hit includes the App telemetry entry", () => {
    const result = settingsEntriesMatching("telemetry");
    expect(result.some((e) => e.id === "setting:app:telemetry")).toBe(true);
  });

  test("shared keyword ranks the exact-title browser theme above app appearance", () => {
    const result = settingsEntriesMatching("theme");
    const ids = result.map((e) => e.id);
    const themeIndex = ids.indexOf("setting:browser:theme");
    const appearanceIndex = ids.indexOf("setting:app:appearance");
    expect(themeIndex).toBeGreaterThanOrEqual(0);
    expect(appearanceIndex).toBeGreaterThanOrEqual(0);
    expect(themeIndex).toBeLessThan(appearanceIndex);
  });

  test("no-match query returns empty array", () => {
    // A token that is not even a spanning subsequence of any entry's text (the
    // most permissive, 80-point branch), so every entry is rejected. A run of
    // repeated letters like "zzzzzz" would NOT work here: it IS a subsequence of
    // several corpus strings, and the crate scorer would match it too.
    expect(settingsEntriesMatching("jjqqxxzzkkvv")).toEqual([]);
  });

  test("corpus sizes guard transcription completeness", () => {
    // 16 sections; 115 setting rows + 1 scroll-speed row = 116 settings.
    expect(sectionEntries).toHaveLength(16);
    expect(settingEntries).toHaveLength(116);
    expect(allEntries).toHaveLength(132);
    // ids are unique across the corpus.
    expect(new Set(allEntries.map((e) => e.id)).size).toBe(allEntries.length);
  });
});
