import { describe, expect, test } from "bun:test";

import {
  shortcutBindingDraftValue,
  shortcutBindingFromDraft,
  shortcutBindingFromKeyboardEvent,
} from "./shortcutBindingEdit";

describe("shortcut binding edit helpers", () => {
  test("draft values expose the raw editable config shape", () => {
    expect(shortcutBindingDraftValue("cmd+t")).toBe("cmd+t");
    expect(shortcutBindingDraftValue(["cmd+k", "cmd+s"])).toBe("cmd+k, cmd+s");
    expect(shortcutBindingDraftValue(null)).toBe("");
    expect(shortcutBindingDraftValue(undefined)).toBe("");
  });

  test("blank drafts explicitly unbind the action", () => {
    expect(shortcutBindingFromDraft("")).toBeNull();
    expect(shortcutBindingFromDraft("   ")).toBeNull();
  });

  test("single-stroke drafts save as strings", () => {
    expect(shortcutBindingFromDraft(" cmd+t ")).toBe("cmd+t");
  });

  test("comma-separated drafts save as shortcut chords", () => {
    expect(shortcutBindingFromDraft("cmd+k, cmd+s")).toEqual(["cmd+k", "cmd+s"]);
    expect(shortcutBindingFromDraft("cmd+k, , cmd+s")).toEqual(["cmd+k", "cmd+s"]);
  });

  test("keyboard recorder captures modified keys in config syntax", () => {
    expect(
      shortcutBindingFromKeyboardEvent({
        key: "K",
        ctrlKey: true,
        shiftKey: true,
      }),
    ).toBe("ctrl+shift+k");
    expect(
      shortcutBindingFromKeyboardEvent({
        key: "ArrowLeft",
        metaKey: true,
        altKey: true,
      }),
    ).toBe("alt+cmd+arrowleft");
  });

  test("keyboard recorder captures parser-supported non-character keys", () => {
    expect(shortcutBindingFromKeyboardEvent({ key: "F5" })).toBe("f5");
    expect(shortcutBindingFromKeyboardEvent({ key: " ", metaKey: true })).toBe(
      "cmd+space",
    );
    expect(shortcutBindingFromKeyboardEvent({ key: "Enter", ctrlKey: true })).toBe(
      "ctrl+enter",
    );
  });

  test("keyboard recorder leaves plain typing to the raw text editor", () => {
    expect(shortcutBindingFromKeyboardEvent({ key: "x" })).toBeUndefined();
    expect(shortcutBindingFromKeyboardEvent({ key: "Shift" })).toBeUndefined();
    expect(shortcutBindingFromKeyboardEvent({ key: "Escape" })).toBeUndefined();
  });

  test("keyboard recorder clears bindings with deletion keys", () => {
    expect(shortcutBindingFromKeyboardEvent({ key: "Backspace" })).toBeNull();
    expect(shortcutBindingFromKeyboardEvent({ key: "Delete" })).toBeNull();
  });
});
