// Presentational, CONTROLLED Settings form. Renders the Sidebar / Notifications
// toggles, the Appearance radio group, and the Shortcuts list straight from a
// `Config` prop, and emits each mutation as a `ConfigAction` through
// `onAction`. It owns NO state and performs NO IPC — the parent (`useConfig`)
// runs the SAME action through the pure `configReducer` (optimistic state) AND
// `deltaForAction` (the persisted dotted-path write), so state and file can
// never disagree about what changed.

import { useState } from "react";

import type { Appearance, Config, ShortcutBinding } from "@cmux/core-types";

import type {
  ConfigAction,
  NotificationsBoolKey,
  SidebarBoolKey,
} from "../settings/configReducer";
import {
  settingsEntriesMatching,
  type SettingsNavigationTarget,
  type SettingsSearchEntry,
} from "../settings/settingsSearch";

export interface SettingsPaneProps {
  config: Config;
  onAction: (action: ConfigAction) => void;
}

interface FlagRow<K> {
  key: K;
  label: string;
}

// Order + labels mirror Settings > Sidebar. Only boolean flags are toggles;
// `branchLayout` and `right_max_width` are edited elsewhere.
const SIDEBAR_FLAGS: ReadonlyArray<FlagRow<SidebarBoolKey>> = [
  { key: "hideAllDetails", label: "Hide all details" },
  { key: "wrapWorkspaceTitles", label: "Wrap workspace titles" },
  { key: "showWorkspaceDescription", label: "Show workspace description" },
  { key: "showNotificationMessage", label: "Show notification message" },
  { key: "showBranchDirectory", label: "Show branch directory" },
  { key: "showPullRequests", label: "Show pull requests" },
  { key: "watchGitStatus", label: "Watch git status" },
  { key: "makePullRequestsClickable", label: "Make pull requests clickable" },
  {
    key: "openPullRequestLinksInCmuxBrowser",
    label: "Open pull request links in cmux browser",
  },
  { key: "openPortLinksInCmuxBrowser", label: "Open port links in cmux browser" },
  { key: "showSSH", label: "Show SSH" },
  { key: "showPorts", label: "Show ports" },
  { key: "showLog", label: "Show log" },
  { key: "showProgress", label: "Show progress" },
  { key: "showCustomMetadata", label: "Show custom metadata" },
];

// Order + labels mirror Settings > Notifications (boolean flags only).
const NOTIFICATION_FLAGS: ReadonlyArray<FlagRow<NotificationsBoolKey>> = [
  { key: "dockBadge", label: "Dock badge" },
  { key: "showInMenuBar", label: "Show in menu bar" },
  { key: "unreadPaneRing", label: "Unread pane ring" },
  { key: "paneFlash", label: "Pane flash" },
];

const APPEARANCE_OPTIONS: ReadonlyArray<{ value: Appearance; label: string }> = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

// Canonical navigation targets → the pane's mounted sections. Only `app`
// has a mounted counterpart today (the Appearance section); the rest of the
// 17 canonical panes land with E11 and extend this map as they mount.
const MOUNTED_SECTION_BY_TARGET: Partial<Record<SettingsNavigationTarget, string>> = {
  app: "appearance",
};

/**
 * Pure results list for the settings search (E8) — split from the input so
 * the ranking render is SSR-testable. `onPick` receives the entry; the
 * default pane handler scrolls to the entry's mounted section when one
 * exists (unmounted canonical panes are inert until E11 builds them).
 */
export function SettingsSearchResults({
  entries,
  onPick,
}: {
  entries: readonly SettingsSearchEntry[];
  onPick?: (entry: SettingsSearchEntry) => void;
}) {
  if (entries.length === 0) {
    return <div className="cmux-settings-search-empty">No matching settings</div>;
  }
  return (
    <ul className="cmux-settings-search-results">
      {entries.map((entry) => (
        <li key={entry.id}>
          <button
            type="button"
            className="cmux-settings-search-result"
            data-target={entry.target}
            data-kind={entry.kind}
            onClick={() => onPick?.(entry)}
          >
            <span className="cmux-settings-search-result-title">{entry.title}</span>
            {entry.subtitle ? (
              <span className="cmux-settings-search-result-subtitle">
                {entry.subtitle}
              </span>
            ) : null}
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The search box + live-ranked results over the canonical settings corpus. */
function SettingsSearch() {
  const [query, setQuery] = useState("");
  const trimmed = query.trim();
  return (
    <div className="cmux-settings-search">
      <input
        type="search"
        className="cmux-settings-search-input"
        placeholder="Search settings"
        aria-label="Search settings"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
      />
      {trimmed !== "" ? (
        <SettingsSearchResults
          entries={settingsEntriesMatching(trimmed)}
          onPick={(entry) => {
            const section = MOUNTED_SECTION_BY_TARGET[entry.target];
            if (section) {
              document
                .querySelector(`[data-section="${section}"]`)
                ?.scrollIntoView({ block: "start" });
            }
          }}
        />
      ) : null}
    </div>
  );
}

/** Render a `ShortcutBinding` (single stroke, chord array, or unbound). */
function formatBinding(binding: ShortcutBinding | null | undefined): string {
  if (binding == null) {
    return "Unbound";
  }
  return Array.isArray(binding) ? binding.join(" ") : binding;
}

export function SettingsPane({ config, onAction }: SettingsPaneProps) {
  const { sidebar, notifications, app, shortcuts } = config;

  return (
    <div className="cmux-settings-pane">
      <SettingsSearch />
      {sidebar && (
        <section className="cmux-settings-section" data-section="sidebar">
          <h2>Sidebar</h2>
          {SIDEBAR_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-field={key}
                checked={sidebar[key]}
                onChange={() => onAction({ type: "toggleSidebarFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
        </section>
      )}

      {notifications && (
        <section className="cmux-settings-section" data-section="notifications">
          <h2>Notifications</h2>
          {NOTIFICATION_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-field={key}
                checked={notifications[key]}
                onChange={() => onAction({ type: "toggleNotificationsFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
        </section>
      )}

      {app && (
        <section className="cmux-settings-section" data-section="appearance">
          <h2>Appearance</h2>
          {APPEARANCE_OPTIONS.map(({ value, label }) => (
            <label key={value} className="cmux-settings-row">
              <input
                type="radio"
                name="appearance"
                data-appearance={value}
                checked={app.appearance === value}
                onChange={() => onAction({ type: "setAppearance", appearance: value })}
              />
              <span>{label}</span>
            </label>
          ))}
        </section>
      )}

      {shortcuts && (
        <section className="cmux-settings-section" data-section="shortcuts">
          <h2>Shortcuts</h2>
          <ul className="cmux-settings-shortcuts">
            {Object.keys(shortcuts.bindings).map((actionId) => (
              <li key={actionId} className="cmux-settings-row" data-action={actionId}>
                <span className="cmux-settings-shortcut-action">{actionId}</span>
                <span className="cmux-settings-shortcut-binding">
                  {formatBinding(shortcuts.bindings[actionId])}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
