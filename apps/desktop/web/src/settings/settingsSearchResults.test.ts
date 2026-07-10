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
  test("account and mobile targets map to their rendered panes", () => {
    expect(paneSectionForEntry(entryById("section:account"))).toBe("account");
    expect(paneSectionForEntry(entryById("setting:account:account"))).toBe(
      "account",
    );
    expect(paneSectionForEntry(entryById("section:mobile"))).toBe("mobile");
  });

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

  test("browser targets map to the rendered browser pane", () => {
    expect(paneSectionForEntry(entryById("section:browser"))).toBe("browser");
    expect(paneSectionForEntry(entryById("setting:browser:theme"))).toBe(
      "browser",
    );
  });

  test("browser import maps to its support pane", () => {
    expect(paneSectionForEntry(entryById("section:browserImport"))).toBe(
      "browserImport",
    );
  });

  test("automation targets map to the rendered automation pane", () => {
    expect(paneSectionForEntry(entryById("section:automation"))).toBe(
      "automation",
    );
    expect(
      paneSectionForEntry(entryById("setting:automation:socket-mode")),
    ).toBe("automation");
    expect(paneSectionForEntry(entryById("setting:automation:codex"))).toBe(
      "automation",
    );
    expect(paneSectionForEntry(entryById("setting:automation:opencode"))).toBe(
      "automation",
    );
    expect(paneSectionForEntry(entryById("setting:automation:port-base"))).toBe(
      "automation",
    );
  });

  test("terminal and TextBox targets map to the rendered terminal pane", () => {
    expect(paneSectionForEntry(entryById("section:terminal"))).toBe("terminal");
    expect(paneSectionForEntry(entryById("setting:terminal:copy-on-select"))).toBe(
      "terminal",
    );
    expect(
      paneSectionForEntry(entryById("setting:textBox:textbox-max-lines")),
    ).toBe("terminal");
  });

  test("workspaceColors targets map to the rendered workspace colors pane", () => {
    expect(paneSectionForEntry(entryById("section:workspaceColors"))).toBe(
      "workspaceColors",
    );
    expect(
      paneSectionForEntry(entryById("setting:workspaceColors:indicator")),
    ).toBe("workspaceColors");
    expect(paneSectionForEntry(entryById("setting:workspaceColors:palette"))).toBe(
      "workspaceColors",
    );
  });

  test("app target rows and the App section map to the rendered app pane", () => {
    expect(paneSectionForEntry(entryById("setting:app:appearance"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("setting:app:telemetry"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("setting:app:new-workspace-placement"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("setting:app:markdown-font-size"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("setting:app:file-editor-word-wrap"))).toBe(
      "appearance",
    );
    expect(paneSectionForEntry(entryById("setting:app:canvas-pane-gap"))).toBe(
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

  test("support-only targets map to rendered status/action panes", () => {
    expect(paneSectionForEntry(entryById("section:customSidebars"))).toBe(
      "customSidebars",
    );
    expect(paneSectionForEntry(entryById("section:betaFeatures"))).toBe(
      "betaFeatures",
    );
    expect(paneSectionForEntry(entryById("section:globalHotkey"))).toBe(
      "globalHotkey",
    );
    expect(paneSectionForEntry(entryById("section:settingsJSON"))).toBe(
      "settingsJSON",
    );
    expect(paneSectionForEntry(entryById("section:reset"))).toBe("reset");
  });
});

// ---- settingsSearchResults producer passthrough --------------------------------

describe("settingsSearchResults", () => {
  test("preserves producer order and entry fields (copy on select)", () => {
    // Mirrors cmuxTests/SettingsSearchIndexTests.swift:57.
    const results = settingsSearchResults("copy on select");
    expect(results[0].id).toBe("setting:terminal:copy-on-select");
    expect(results[0].paneSection).toBe("terminal");
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
      expect(result.paneSection).not.toBeNull();
    }
  });

  test("empty query returns the 16 sections (producer contract)", () => {
    const results = settingsSearchResults("");
    expect(results).toHaveLength(16);
    expect(results[0].id).toBe("section:account");
    expect(results[0].paneSection).toBe("account");
  });
});
