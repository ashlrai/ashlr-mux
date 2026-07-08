import { describe, expect, test } from "bun:test";

import {
  paneSectionForEntry,
  settingsSearchResults,
} from "./settingsSearchResults";
import { allEntries, SETTINGS_NAVIGATION_TARGETS } from "./settingsSearch";

/** Entry lookup by id from the full corpus (ids are unique). */
function entryById(id: string) {
  const entry = allEntries.find((e) => e.id === id);
  if (!entry) {
    throw new Error(`no corpus entry with id ${id}`);
  }
  return entry;
}

// ---- paneSectionForEntry mapping ---------------------------------------------

describe("paneSectionForEntry", () => {
  test("sidebarAppearance target maps to sidebar (rows and section)", () => {
    expect(
      paneSectionForEntry(entryById("setting:sidebarAppearance:show-ports")),
    ).toBe("sidebar");
    expect(paneSectionForEntry(entryById("section:sidebarAppearance"))).toBe(
      "sidebar",
    );
  });

  test("keyboardShortcuts target maps to shortcuts (rows and section)", () => {
    expect(paneSectionForEntry(entryById("section:keyboardShortcuts"))).toBe(
      "shortcuts",
    );
    expect(
      paneSectionForEntry(entryById("setting:keyboardShortcuts:shortcuts")),
    ).toBe("shortcuts");
  });

  test("appearance row and the App section map to appearance", () => {
    expect(paneSectionForEntry(entryById("setting:app:appearance"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("section:app"))).toBe("appearance");
  });

  test("the four notification toggles map to notifications", () => {
    for (const id of [
      "setting:app:dock-badge",
      "setting:app:show-menu-bar",
      "setting:app:unread-pane-ring",
      "setting:app:pane-flash",
    ]) {
      expect(paneSectionForEntry(entryById(id))).toBe("notifications");
    }
  });

  test("targets the pane does not render map to null", () => {
    expect(paneSectionForEntry(entryById("setting:app:telemetry"))).toBeNull();
    expect(paneSectionForEntry(entryById("section:terminal"))).toBeNull();
    expect(
      paneSectionForEntry(entryById("setting:automation:socket-mode")),
    ).toBeNull();
  });
});

// ---- settingsSearchResults producer passthrough --------------------------------

describe("settingsSearchResults", () => {
  test("preserves producer order and entry fields (copy on select)", () => {
    // Mirrors cmuxTests/SettingsSearchIndexTests.swift:57.
    const results = settingsSearchResults("copy on select");
    expect(results[0].id).toBe("setting:terminal:copy-on-select");
    expect(results[0].paneSection).toBeNull();
    expect(results[0].subtitle).toBe("Terminal");
  });

  test("stop-words-only query returns the 16 sections in allCases order", () => {
    const results = settingsSearchResults("settings");
    expect(results).toHaveLength(16);
    expect(results.map((r) => r.id)).toEqual(
      SETTINGS_NAVIGATION_TARGETS.map((target) => `section:${target}`),
    );
    for (const result of results) {
      expect(result.subtitle).toBeNull();
    }
  });

  test("empty query returns the 16 sections (producer contract)", () => {
    const results = settingsSearchResults("");
    expect(results).toHaveLength(16);
    expect(results[0].id).toBe("section:account");
  });
});
