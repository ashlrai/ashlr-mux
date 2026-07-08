// Presentational, CONTROLLED Settings form. Renders the Sidebar / Notifications
// toggles, the Appearance radio group, and the Shortcuts list straight from a
// `Config` prop, and emits the next `Config` through `onChange`. It owns NO
// state and performs NO IPC — the parent holds the config and persists it. All
// mutations go through the pure `configReducer`, mirroring how the macOS
// Settings panes mutate the in-memory `CmuxConfigFile`. The search box is
// controlled like everything else (`searchQuery` / `onSearchQueryChange`);
// navigation (scroll-to-section) is the parent's side effect via `onNavigate`.

import type { Appearance, Config } from "@cmux/core-types";

import {
  configReducer,
  type NotificationsBoolKey,
  type SidebarBoolKey,
} from "../settings/configReducer";
import { shortcutBindingDisplayString } from "../settings/shortcutBinding";
import {
  settingsSearchResults,
  type SettingsPaneSection,
} from "../settings/settingsSearchResults";

export interface SettingsPaneProps {
  config: Config;
  onChange: (next: Config) => void;
  searchQuery?: string;
  onSearchQueryChange?: (query: string) => void;
  onNavigate?: (section: SettingsPaneSection) => void;
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

export function SettingsPane({
  config,
  onChange,
  searchQuery,
  onSearchQueryChange,
  onNavigate,
}: SettingsPaneProps) {
  const { sidebar, notifications, app, shortcuts } = config;
  // Trimmed-empty restores the plain section state (SettingsWindowScene.swift:143).
  const isSearching = (searchQuery ?? "").trim() !== "";

  const searchInput = (
    <input
      type="search"
      className="cmux-settings-search"
      data-settings-search
      value={searchQuery ?? ""}
      placeholder="Search"
      aria-label="Search settings"
      onChange={(e) => onSearchQueryChange?.(e.target.value)}
    />
  );

  if (isSearching) {
    // The flat pane is both results list and detail: while searching, only the
    // hits render. Canonical lists ALL matches; rows whose target has no web
    // pane section are disabled. Clicking clears the query (restoring the
    // pane), then navigates to the hit's parent section — the web adaptation
    // of `selectSidebarEntry` (SettingsWindowScene.swift:230-245).
    const results = settingsSearchResults(searchQuery ?? "");
    return (
      <div className="cmux-settings-pane">
        {searchInput}
        {results.length === 0 ? (
          <p className="cmux-settings-search-empty">No Results</p>
        ) : (
          <ul className="cmux-settings-search-results">
            {results.map((result) => (
              <li
                key={result.id}
                className="cmux-settings-row"
                data-result-id={result.id}
              >
                <button
                  type="button"
                  disabled={result.paneSection == null}
                  onClick={() => {
                    onSearchQueryChange?.("");
                    if (result.paneSection) {
                      onNavigate?.(result.paneSection);
                    }
                  }}
                >
                  <span className="cmux-settings-search-result-title">
                    {result.title}
                  </span>
                  {result.subtitle != null && (
                    <span className="cmux-settings-search-result-section">
                      {result.subtitle}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    );
  }

  return (
    <div className="cmux-settings-pane">
      {searchInput}
      {sidebar && (
        <section className="cmux-settings-section" data-section="sidebar">
          <h2>Sidebar</h2>
          {SIDEBAR_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-field={key}
                checked={sidebar[key]}
                onChange={() =>
                  onChange(configReducer(config, { type: "toggleSidebarFlag", key }))
                }
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
                onChange={() =>
                  onChange(
                    configReducer(config, { type: "toggleNotificationsFlag", key }),
                  )
                }
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
                onChange={() =>
                  onChange(configReducer(config, { type: "setAppearance", appearance: value }))
                }
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
                  {shortcutBindingDisplayString(actionId, shortcuts.bindings[actionId])}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
