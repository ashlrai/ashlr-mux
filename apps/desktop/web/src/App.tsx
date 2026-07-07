import { useState } from "react";

import { Icon } from "@cmux/webviews/src/icons";

import { CommandPaletteOverlay } from "./components/CommandPaletteOverlay";
import { Sidebar } from "./components/Sidebar";
import { Workspace } from "./components/Workspace";

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
      </header>
      <div className="cmux-body">
        <Sidebar collapsed={sidebarCollapsed} />
        <main className="cmux-main">
          <Workspace />
        </main>
      </div>
      <CommandPaletteOverlay />
    </div>
  );
}
