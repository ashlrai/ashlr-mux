// Projection of `settingsEntriesMatching` hits onto the web SettingsPane.
// Canonical macOS search (SettingsWindowScene.swift:176-245) navigates a hit to
// its parent SECTION. The web pane renders an anchor for every supported target;
// editable config sections expose controls, while platform/backend gaps land on
// status or action rows.

import {
  settingsEntriesMatching,
  type SettingsSearchEntry,
} from "./settingsSearch";

/** The `data-section` anchor values rendered by SettingsPane. */
export type SettingsPaneSection =
  | "account"
  | "mobile"
  | "updates"
  | "customSidebars"
  | "betaFeatures"
  | "automation"
  | "browser"
  | "browserImport"
  | "globalHotkey"
  | "terminal"
  | "workspaceColors"
  | "sidebar"
  | "notifications"
  | "appearance"
  | "shortcuts"
  | "settingsJSON"
  | "reset";

export interface SettingsSearchResult {
  id: string;
  title: string;
  subtitle: string | null;
  paneSection: SettingsPaneSection | null;
}

// The 4 App-target rows the pane renders under Notifications (they are exactly
// the NOTIFICATION_FLAGS toggles in SettingsPane.tsx).
const NOTIFICATION_ENTRY_IDS: ReadonlySet<string> = new Set([
  "setting:app:dock-badge",
  "setting:app:show-menu-bar",
  "setting:app:unread-pane-ring",
  "setting:app:pane-flash",
]);

/**
 * Where in the web pane a hit navigates. Entry-id overrides come first (the
 * App target scatters across pane sections), then whole targets whose parent
 * section the pane renders; everything else is non-navigable (`null`).
 * `app` -> "appearance" is a documented projection choice: the web section
 * still uses the original appearance anchor for compatibility, but now renders
 * the broader App config slice.
 */
export function paneSectionForEntry(
  entry: SettingsSearchEntry,
): SettingsPaneSection | null {
  if (NOTIFICATION_ENTRY_IDS.has(entry.id)) {
    return "notifications";
  }
  if (entry.target === "app") {
    return "appearance";
  }
  if (entry.target === "terminal" || entry.target === "textBox") {
    return "terminal";
  }
  if (entry.target === "sidebarAppearance") {
    return "sidebar";
  }
  if (entry.target === "keyboardShortcuts") {
    return "shortcuts";
  }
  if (entry.target === "settingsJSON") {
    return "settingsJSON";
  }
  if (entry.target === "customSidebars") {
    return "customSidebars";
  }
  if (entry.target === "betaFeatures") {
    return "betaFeatures";
  }
  if (entry.target === "browserImport") {
    return "browserImport";
  }
  if (entry.target === "globalHotkey") {
    return "globalHotkey";
  }
  if (entry.target === "reset") {
    return "reset";
  }
  if (
    entry.target === "account" ||
    entry.target === "mobile" ||
    entry.target === "automation" ||
    entry.target === "browser" ||
    entry.target === "workspaceColors"
  ) {
    return entry.target;
  }
  return null;
}

/**
 * Producer hits projected for rendering, preserving producer order exactly
 * (score ascending, original offset as tiebreak).
 */
export function settingsSearchResults(query: string): SettingsSearchResult[] {
  return settingsEntriesMatching(query).map((entry) => ({
    id: entry.id,
    title: entry.title,
    subtitle: entry.subtitle,
    paneSection: paneSectionForEntry(entry),
  }));
}
