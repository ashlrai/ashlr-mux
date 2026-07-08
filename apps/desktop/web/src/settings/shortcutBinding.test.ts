// Oracle tests for shortcutBinding.ts, pinned to the Swift config-parse
// semantics of `Sources/KeyboardShortcutSettings.swift:2385-2552` and
// `Sources/KeyboardShortcutSettingsFileStore.swift:994-1027`. The end-to-end
// rows exercise `shortcutBindingDisplayString`, the exact projection
// SettingsPane renders.

import { describe, expect, test } from "bun:test";

import {
  BARE_FIRST_STROKE_ACTIONS,
  NUMBERED_DIGIT_ACTIONS,
  isUnboundConfigToken,
  parseConfigKeyToken,
  parseConfigStroke,
  parseStoredShortcut,
  shortcutBindingDisplayString,
} from "./shortcutBinding";
import { UNBOUND_SHORTCUT } from "./shortcutFormat";

// A plain action: not numbered, no bare first stroke allowed.
const PLAIN = "workspace.new";

describe("parseConfigKeyToken — alias table (Swift lines 2444-2513)", () => {
  const rows: Array<[string, string | null]> = [
    ["arrowleft", "←"],
    ["LeftArrow", "←"], // lowercased before matching
    ["→", "→"],
    ["up", "↑"],
    ["down", "↓"],
    ["tab", "\t"],
    ["enter", "\r"],
    ["return", "\r"],
    ["↩", "\r"],
    ["spacebar", "space"],
    ["<space>", "space"],
    ["comma", ","],
    ["dot", "."],
    ["slash", "/"],
    ["backslash", "\\"],
    ["semicolon", ";"],
    ["apostrophe", "'"],
    ["grave", "`"],
    ["backtick", "`"],
    ["hyphen", "-"],
    ["equals", "="],
    ["openbracket", "["],
    ["closebracket", "]"],
    ["volumeup", "media.volumeUp"],
    ["mediavolumedown", "media.volumeDown"],
    ["media.brightnessup", "media.brightnessUp"],
    ["brightnessdown", "media.brightnessDown"],
    ["mute", "media.mute"],
    ["playpause", "media.playPause"],
    ["medianext", "media.next"],
    ["media.previoustrack", "media.previous"],
    // F-key rule: Swift Int parse of the remainder in 1...20.
    ["f5", "f5"],
    ["F12", "f12"], // capital F lowercased first
    ["f05", "f5"], // Swift Int("05") == 5
    ["f21", null], // out of 1...20; "f21" is also not a single character
    // Default arms.
    ["a", "a"],
    ["P", "p"],
    ["  x  ", "x"], // trimmed before matching
    ["ab", null],
    ["", null],
    [" ", "space"], // literal single space is the space key
    ["   ", null], // other all-whitespace tokens are invalid
  ];
  test.each(rows)("%j → %j", (raw, expected) => {
    expect(parseConfigKeyToken(raw)).toBe(expected);
  });
});

describe("parseConfigStroke — modifiers and raw-last-part rules (Swift lines 2385-2423)", () => {
  test("modifier aliases including glyph forms", () => {
    expect(parseConfigStroke("⌘+p")).toEqual({
      key: "p",
      command: true,
      shift: false,
      option: false,
      control: false,
    });
    expect(parseConfigStroke("ctl+alt+⇧+cmd+k")).toEqual({
      key: "k",
      command: true,
      shift: true,
      option: true,
      control: true,
    });
  });

  test("unknown modifier invalidates the stroke", () => {
    expect(parseConfigStroke("meta+p")).toBeNull();
  });

  test('"cmd+ " keeps the RAW last part → space key; "cmd+" has an empty last part → invalid', () => {
    expect(parseConfigStroke("cmd+ ")).toEqual({
      key: "space",
      command: true,
      shift: false,
      option: false,
      control: false,
    });
    expect(parseConfigStroke("cmd+")).toBeNull();
    expect(parseConfigStroke("")).toBeNull();
  });
});

describe("isUnboundConfigToken (Swift lines 2546-2552)", () => {
  const rows: Array<[string, boolean]> = [
    ["", true],
    ["none", true],
    ["NONE", true],
    [" Clear ", true],
    ["unbound", true],
    ["Disabled", true],
    ["\n", true], // trims to empty
    [" ", false], // a literal single space is the space key, not an unbind
    ["cmd+t", false],
  ];
  test.each(rows)("%j → %p", (raw, expected) => {
    expect(isUnboundConfigToken(raw)).toBe(expected);
  });
});

describe("parseStoredShortcut — chord and bare-first-stroke guards (Swift lines 2524-2536)", () => {
  test("bare first stroke rejected by default, allowed when opted in", () => {
    expect(parseStoredShortcut(["p"])).toBeNull();
    expect(parseStoredShortcut(["j"], true)).toEqual({
      first: { key: "j", command: false, shift: false, option: false, control: false },
    });
  });

  test('bare " " passes the guard only because its key is "space"', () => {
    expect(parseStoredShortcut([" "])).toEqual({
      first: { key: "space", command: false, shift: false, option: false, control: false },
    });
  });

  test("only the FIRST stroke needs a modifier; the second may be bare", () => {
    expect(parseStoredShortcut(["cmd+k", "1"])).toEqual({
      first: { key: "k", command: true, shift: false, option: false, control: false },
      second: { key: "1", command: false, shift: false, option: false, control: false },
    });
  });

  test("a malformed chord half invalidates the whole binding", () => {
    expect(parseStoredShortcut(["cmd+k", "nope"])).toBeNull();
  });

  test("empty and 3-element stroke arrays are invalid; lone unbind token is unbound", () => {
    expect(parseStoredShortcut([])).toBeNull();
    expect(parseStoredShortcut(["cmd+a", "b", "c"])).toBeNull();
    expect(parseStoredShortcut(["none"])).toBe(UNBOUND_SHORTCUT);
  });
});

describe("shortcutBindingDisplayString — end-to-end projection", () => {
  const rows: Array<[string, string | string[] | null | undefined, string]> = [
    // Single strokes.
    [PLAIN, "cmd+t", "⌘T"],
    [PLAIN, "cmd+enter", "⌘↩"],
    [PLAIN, "ctrl+opt+shift+cmd+f5", "⌃⌥⇧⌘F5"],
    [PLAIN, "cmd+ ", "⌘Space"],
    [PLAIN, " ", "Space"],
    // Unbound forms → "None".
    [PLAIN, null, "None"],
    [PLAIN, undefined, "None"],
    [PLAIN, [], "None"],
    [PLAIN, "none", "None"],
    [PLAIN, "CLEAR", "None"],
    [PLAIN, " unbound ", "None"],
    [PLAIN, "Disabled", "None"],
    // Chords.
    [PLAIN, ["cmd+k", "1"], "⌘K 1"],
    [PLAIN, ["cmd+k", "cmd+s"], "⌘K ⌘S"],
    // Numbered actions render the digit as the 1…9 range.
    ["selectSurfaceByNumber", "ctrl+3", "⌃1…9"],
    ["selectWorkspaceByNumber", ["cmd+k", "2"], "⌘K 1…9"],
    // Numbered action with a non-digit key falls through to plain rendering.
    ["selectSurfaceByNumber", "cmd+t", "⌘T"],
    // Bare first stroke: rejected for plain actions, allowed for diff viewer.
    ["diffViewerScrollDown", "j", "J"],
    // Invalid values fall back to the raw config text (canonical would fall
    // back to the built-in default binding; no defaults catalog yet).
    [PLAIN, "p", "p"],
    [PLAIN, "cmd+", "cmd+"],
    [PLAIN, ["cmd", "w"], "cmd w"], // "cmd" alone is not a key token
    [PLAIN, ["cmd+a", "b", "c"], "cmd+a b c"],
  ];
  test.each(rows)("(%j, %j) → %j", (actionId, binding, expected) => {
    expect(shortcutBindingDisplayString(actionId, binding)).toBe(expected);
  });
});

describe("per-action metadata (Swift lines 582-604)", () => {
  test("numbered and bare-first-stroke action sets match the oracle", () => {
    expect([...NUMBERED_DIGIT_ACTIONS].sort()).toEqual([
      "selectSurfaceByNumber",
      "selectWorkspaceByNumber",
    ]);
    expect([...BARE_FIRST_STROKE_ACTIONS].sort()).toEqual(
      [
        "diffViewerScrollDown",
        "diffViewerScrollUp",
        "diffViewerScrollToBottom",
        "diffViewerScrollToTop",
        "diffViewerOpenFileSearch",
        "fileExplorerOpenSelection",
        "fileExplorerOpenSelectionFinderAlias",
      ].sort(),
    );
  });
});
