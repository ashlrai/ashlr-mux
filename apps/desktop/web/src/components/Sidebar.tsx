// The left sessions/workspace sidebar — the functional-parity first cut of
// cmux's `CmuxSidebar` / sessions index. Renders the first window's live
// workspace list (from the Rust session snapshot) as selectable rows, with a
// "new workspace" (+) control and a per-row close (✕) on hover.
//
// Data + mutations come from `useSession`; every `useSession` instance stays in
// sync through the broadcast `cmux://session-changed` event, so this sidebar and
// the main `Workspace` reconcile to the same snapshot after any change.

import { Icon } from "@cmux/webviews/src/icons";

import { useSession } from "../hooks/useSession";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

/** The label shown for a workspace row: its custom title, else process title. */
function workspaceTitle(ws: SessionWorkspaceSnapshot): string {
  return ws.custom_title?.trim() || ws.process_title?.trim() || "Terminal";
}

export interface SidebarProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
}

export function Sidebar({ collapsed }: SidebarProps): React.JSX.Element {
  const {
    workspaces,
    selectedWorkspaceIndex,
    newWorkspace,
    selectWorkspace,
    closeWorkspace,
  } = useSession();

  if (collapsed) {
    return <div className="cmux-sidebar cmux-sidebar--collapsed" aria-hidden="true" />;
  }

  return (
    <nav className="cmux-sidebar" aria-label="Workspaces">
      <div className="cmux-sidebar-header">
        <span className="cmux-sidebar-title">Workspaces</span>
        <button
          type="button"
          className="cmux-sidebar-new"
          title="New workspace"
          aria-label="New workspace"
          onClick={() => newWorkspace()}
        >
          +
        </button>
      </div>
      <ul className="cmux-sidebar-list">
        {workspaces.map((ws, index) => {
          const selected = index === selectedWorkspaceIndex;
          return (
            <li
              key={ws.workspace_id ?? `ws-${index}`}
              className={
                selected
                  ? "cmux-sidebar-row is-selected"
                  : "cmux-sidebar-row"
              }
              aria-selected={selected}
              onClick={() => selectWorkspace(index)}
            >
              <span className="cmux-icon cmux-sidebar-row-icon" aria-hidden="true">
                <Icon name="classic" />
              </span>
              <span className="cmux-sidebar-row-label">{workspaceTitle(ws)}</span>
              <button
                type="button"
                className="cmux-sidebar-row-close"
                title="Close workspace"
                aria-label={`Close ${workspaceTitle(ws)}`}
                onClick={(event) => {
                  // Don't let the row's select handler fire on close.
                  event.stopPropagation();
                  closeWorkspace(index);
                }}
              >
                ✕
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
