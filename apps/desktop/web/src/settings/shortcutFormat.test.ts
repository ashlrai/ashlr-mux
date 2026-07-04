// Oracle tests for shortcutFormat.ts, pinned to Swift semantics.
//
// The first block ports all 7 cases from
// `Packages/macOS/CmuxSettingsUI/Tests/CmuxSettingsUITests/ShortcutDisplayStringTests.swift:1-68`
// verbatim; the remaining tables are hand-derived from
// `ShortcutDisplayFormatter.swift:1-165`.

import { describe, expect, test } from "bun:test";

import {
  NUMBERED_DIGIT_RANGE_HINT,
  UNBOUND_SHORTCUT,
  isNumberedDigitKey,
  isUnbound,
  keyDisplayString,
  modifierDisplayString,
  shortcutDisplayString,
  strokeDisplayString,
  swiftParseInt,
  type ShortcutStroke,
  type StoredShortcut,
} from "./shortcutFormat";

function single(first: ShortcutStroke): StoredShortcut {
  return { first };
}

function chord(first: ShortcutStroke, second: ShortcutStroke): StoredShortcut {
  return { first, second };
}

describe("shortcutDisplayString — Swift oracle (ShortcutDisplayStringTests.swift)", () => {
  test("numbered control digit renders as range (issue #5189 regression)", () => {
    expect(shortcutDisplayString(single({ key: "1", control: true }), true)).toBe("⌃1…9");
  });

  test("numbered command digit renders as range", () => {
    expect(shortcutDisplayString(single({ key: "1", command: true }), true)).toBe("⌘1…9");
  });

  test("numbered range ignores which digit was recorded", () => {
    expect(shortcutDisplayString(single({ key: "5", option: true }), true)).toBe("⌥1…9");
  });

  test("non-numbered shortcut keeps its literal key", () => {
    expect(shortcutDisplayString(single({ key: "1", command: true }), false)).toBe("⌘1");
    expect(shortcutDisplayString(single({ key: "w", command: true }), false)).toBe("⌘W");
  });

  test("numbered non-digit key falls back to literal", () => {
    expect(shortcutDisplayString(single({ key: "a", control: true }), true)).toBe("⌃A");
  });

  test("numbered zero and out-of-range digits fall back to literal", () => {
    expect(shortcutDisplayString(single({ key: "0", command: true }), true)).toBe("⌘0");
  });

  test("unbound renders as None under both numbered flags", () => {
    expect(shortcutDisplayString(UNBOUND_SHORTCUT, true)).toBe("None");
    expect(shortcutDisplayString(UNBOUND_SHORTCUT, false)).toBe("None");
    // Default `numbered` is false, mirroring the Swift default parameter.
    expect(shortcutDisplayString(UNBOUND_SHORTCUT)).toBe("None");
  });
});

describe("shortcutDisplayString — chords and branch order", () => {
  test("modifier glyphs render in fixed ⌃⌥⇧⌘ order regardless of input", () => {
    expect(
      shortcutDisplayString(
        single({ key: "k", command: true, shift: true, option: true, control: true }),
      ),
    ).toBe("⌃⌥⇧⌘K");
  });

  test("plain chord joins both strokes with a single space", () => {
    expect(
      shortcutDisplayString(chord({ key: "k", command: true }, { key: "s", command: true })),
    ).toBe("⌘K ⌘S");
  });

  test("numbered chord with digit second stroke renders first + range", () => {
    expect(
      shortcutDisplayString(chord({ key: "k", command: true }, { key: "1", control: true }), true),
    ).toBe("⌘K ⌃" + NUMBERED_DIGIT_RANGE_HINT);
  });

  test("numbered chord with NON-digit second key falls through to a plain chord", () => {
    // Swift lines 25-36: the numbered branch checks ONLY the second stroke of
    // a chord; a digit FIRST key never triggers the range in a chord.
    expect(
      shortcutDisplayString(chord({ key: "1", command: true }, { key: "s", command: true }), true),
    ).toBe("⌘1 ⌘S");
  });

  test("empty first key WITH a second stroke is not unbound; renders modifiers only", () => {
    const s = chord({ key: "", control: true }, { key: "x", command: true });
    expect(isUnbound(s)).toBe(false);
    // keyDisplayString("") = "".toUpperCase() = "" → modifiers-only first part.
    expect(shortcutDisplayString(s)).toBe("⌃ ⌘X");
  });
});

describe("keyDisplayString — named keys and media labels", () => {
  const named: Array<[string, string]> = [
    ["\t", "Tab"],
    ["space", "Space"],
    ["\r", "↩"],
    ["media.brightnessDown", "Brightness Down"],
    ["media.brightnessUp", "Brightness Up"],
    ["media.mute", "Mute"],
    ["media.next", "Next Track"],
    ["media.playPause", "Play/Pause"],
    ["media.previous", "Previous Track"],
    ["media.volumeDown", "Volume Down"],
    ["media.volumeUp", "Volume Up"],
  ];
  test.each(named)("%j → %j", (key, expected) => {
    expect(keyDisplayString(key)).toBe(expected);
  });

  test("default arm uppercases with locale-independent Unicode semantics", () => {
    expect(keyDisplayString("a")).toBe("A");
    expect(keyDisplayString("ß")).toBe("SS"); // Swift uppercased() does the same.
    expect(keyDisplayString("←")).toBe("←");
    expect(keyDisplayString("")).toBe("");
  });
});

describe("keyDisplayString — F-key rule (Swift lines 157-164)", () => {
  const fkeys: Array<[string, string]> = [
    ["f1", "F1"],
    ["f5", "F5"],
    ["f05", "F5"], // Swift Int("05") == 5
    ["f+1", "F1"], // Swift Int("+1") == 1
    ["f20", "F20"],
    ["f21", "F21"], // out of 1...20 → uppercase fallback
    ["f0", "F0"], // 0 out of range → uppercase fallback
    ["f", "F"], // Int("") == nil → uppercase fallback
    ["f1x", "F1X"], // Int("1x") == nil → uppercase fallback
    ["F5", "F5"], // capital F does not match hasPrefix("f"); uppercase fallback
  ];
  test.each(fkeys)("%j → %j", (key, expected) => {
    expect(keyDisplayString(key)).toBe(expected);
  });
});

describe("isNumberedDigitKey — Swift Int(key) semantics (lines 152-155)", () => {
  const rows: Array<[string, boolean]> = [
    ["5", true],
    ["1", true],
    ["9", true],
    ["05", true], // Swift Int("05") == 5
    ["+3", true], // Swift Int("+3") == 3
    ["-1", false], // parses to -1, out of 1...9
    ["0", false],
    ["10", false],
    [" 1", false], // whitespace never parses
    ["", false],
    ["٥", false], // non-ASCII digit never parses
    ["1.0", false],
  ];
  test.each(rows)("%j → %p", (key, expected) => {
    expect(isNumberedDigitKey(key)).toBe(expected);
  });
});

describe("swiftParseInt", () => {
  const rows: Array<[string, number | null]> = [
    ["05", 5],
    ["+3", 3],
    ["-7", -7],
    ["0", 0],
    ["20", 20],
    [" 1", null],
    ["1 ", null],
    ["1.0", null],
    ["", null],
    ["+", null],
    ["-", null],
    ["٥", null], // Arabic-Indic five: Swift Int rejects non-ASCII digits
    ["0x5", null],
    ["1_0", null],
    ["++1", null],
    ["00000000000000000005", 5], // leading zeros stripped before overflow gate
  ];
  test.each(rows)("%j → %p", (s, expected) => {
    expect(swiftParseInt(s)).toBe(expected);
  });
});

describe("stroke/modifier helpers", () => {
  test("strokeDisplayString composes modifiers + key label", () => {
    expect(strokeDisplayString({ key: "f5", command: true, shift: true })).toBe("⇧⌘F5");
    expect(strokeDisplayString({ key: "space", control: true })).toBe("⌃Space");
  });

  test("modifierDisplayString with no modifiers is empty", () => {
    expect(modifierDisplayString({ key: "a" })).toBe("");
  });
});
