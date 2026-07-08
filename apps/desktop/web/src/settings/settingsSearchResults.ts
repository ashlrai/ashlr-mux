// Projection of `settingsEntriesMatching` hits onto the web SettingsPane.
// Canonical macOS search (SettingsWindowScene.swift:176-245) navigates a hit to
// its parent SECTION — one of 16 `SettingsNavigationTarget`s. The web pane only
// renders 4 sections, so each entry maps to the pane section it can navigate
// to, or `null` when the pane has no UI for that target (the row still renders,
// mirroring the canonical sidebar listing every match).

import {
  settingsEntriesMatching,
  type SettingsSearchEntry,
} from "./settingsSearch";

/** The `data-section` anchor values rendered by SettingsPane. */
export type SettingsPaneSection =
  | "sidebar"
  | "notifications"
  | "appearance"
  | "shortcuts";

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
 * `section:app` -> "appearance" is a documented projection choice: the
 * Appearance radio group is the App slice's only rendered UI in this pane.
 */
export function paneSectionForEntry(
  entry: SettingsSearchEntry,
): SettingsPaneSection | null {
  if (entry.id === "setting:app:appearance" || entry.id === "section:app") {
    return "appearance";
  }
  if (NOTIFICATION_ENTRY_IDS.has(entry.id)) {
    return "notifications";
  }
  if (entry.target === "sidebarAppearance") {
    return "sidebar";
  }
  if (entry.target === "keyboardShortcuts") {
    return "shortcuts";
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
