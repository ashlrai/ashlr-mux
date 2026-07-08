import { useEffect, useState } from "react";

import { Icon } from "@cmux/webviews/src/icons";

import { CommandPaletteOverlay } from "./components/CommandPaletteOverlay";
import { SettingsOverlay } from "./components/SettingsOverlay";
import { Sidebar } from "./components/Sidebar";
import { Workspace } from "./components/Workspace";
import { useAppearance } from "./hooks/useAppearance";
import { useConfig } from "./hooks/useConfig";

/** Window event that opens Settings (fired by the palette's openSettings). */
export const OPEN_SETTINGS_EVENT = "cmux:open-settings";

/**
 * The app shell. A top bar (sidebar toggle + app identity) over a two-column
 * body: the left {@link Sidebar} (live workspace list) and the main
 * {@link Workspace} (the flat portal of terminal/agent/diff/markdown panes).
 *
 * This is the functional-parity first cut of cmux's window chrome — the OS
 * title bar is still in use (Tauri `decorations: true`); a custom title bar with
 * min/max/close is the next slice.
 */
export function App(): React.JSX.Element {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Live config feeds the applied appearance (E9): the stored app.appearance
  // resolves against the ambient system scheme onto :root, and a legacy
  // stored value ("auto") is normalized back through the same single
  // mutation path the Settings pane uses.
  const { config, dispatch } = useConfig();
  useAppearance(config?.app?.appearance ?? null, (persistedRawValue) =>
    dispatch({
      type: "setAppearance",
      // The resolver never emits "auto" (normalization collapses it to
      // "system") but its type keeps the full mode union — narrow for the
      // config's Appearance union.
      appearance: persistedRawValue === "auto" ? "system" : persistedRawValue,
    }),
  );

  // The command palette (and any future entrypoint) opens Settings through
  // one window event, keeping a single open path without prop-drilling into
  // the palette's intent switch.
  useEffect(() => {
    const open = (): void => setSettingsOpen(true);
    window.addEventListener(OPEN_SETTINGS_EVENT, open);
    return () => window.removeEventListener(OPEN_SETTINGS_EVENT, open);
  }, []);

  return (
    <div className="cmux-shell">
      <header className="cmux-app-header flex items-center gap-2 px-2 py-1.5 text-[13px] text-neutral-300 select-none">
        <button
          type="button"
          className="cmux-icon cmux-sidebar-toggle"
          title={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
          aria-pressed={!sidebarCollapsed}
          onClick={() => setSidebarCollapsed((v) => !v)}
        >
          <Icon name="bars" />
        </button>
        <span className="cmux-icon inline-flex h-4 w-4 text-neutral-400">
          <Icon name="classic" />
        </span>
        <span className="font-medium">cmux for Windows</span>
        <span className="flex-1" />
        <button
          type="button"
          className="cmux-settings-toggle"
          title="Settings"
          aria-label="Settings"
          onClick={() => setSettingsOpen(true)}
        >
          {/* The shared icon set has no gear glyph; the character keeps the
              header dependency-free until one lands. */}
          ⚙
        </button>
      </header>
      <div className="cmux-body">
        <Sidebar collapsed={sidebarCollapsed} />
        <main className="cmux-main">
          <Workspace />
        </main>
      </div>
      <CommandPaletteOverlay />
      <SettingsOverlay open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
