import { describe, expect, test } from "bun:test";

import {
  WARM_CLAUDE_CODE_SHORTCUT_ACTION,
  shortcutActionForEvent,
  shortcutBindingMatchesEvent,
} from "./shortcutRuntime";
import { DEFAULT_SHORTCUTS_CONFIG } from "./defaultConfig";

describe("shortcut runtime matching", () => {
  test("frontend default config exposes the warm-Claude shortcut action", () => {
    expect(
      DEFAULT_SHORTCUTS_CONFIG.bindings[WARM_CLAUDE_CODE_SHORTCUT_ACTION],
    ).toBe("ctrl+alt+c");
  });

  test("matches the default Windows warm-Claude shortcut", () => {
    expect(
      shortcutBindingMatchesEvent("ctrl+alt+c", {
        key: "c",
        ctrlKey: true,
        altKey: true,
      }),
    ).toBe(true);
    expect(
      shortcutBindingMatchesEvent("ctrl+alt+c", {
        key: "C",
        ctrlKey: true,
        altKey: true,
      }),
    ).toBe(true);
  });

  test("requires exact modifiers", () => {
    expect(
      shortcutBindingMatchesEvent("ctrl+alt+c", {
        key: "c",
        ctrlKey: true,
      }),
    ).toBe(false);
    expect(
      shortcutBindingMatchesEvent("ctrl+alt+c", {
        key: "c",
        ctrlKey: true,
        altKey: true,
        shiftKey: true,
      }),
    ).toBe(false);
  });

  test("ignores unbound, invalid, and chord bindings in the single-stroke dispatcher", () => {
    expect(shortcutBindingMatchesEvent(null, { key: "c" })).toBe(false);
    expect(shortcutBindingMatchesEvent("none", { key: "c" })).toBe(false);
    expect(shortcutBindingMatchesEvent("c", { key: "c" })).toBe(false);
    expect(
      shortcutBindingMatchesEvent(["ctrl+alt+c", "x"], {
        key: "c",
        ctrlKey: true,
        altKey: true,
      }),
    ).toBe(false);
  });

  test("resolves the first matching action id from the binding map", () => {
    expect(
      shortcutActionForEvent(
        {
          [WARM_CLAUDE_CODE_SHORTCUT_ACTION]: "ctrl+alt+c",
        },
        {
          key: "c",
          ctrlKey: true,
          altKey: true,
        },
      ),
    ).toBe(WARM_CLAUDE_CODE_SHORTCUT_ACTION);
  });
});
