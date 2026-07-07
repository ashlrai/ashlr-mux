// The left sessions/workspace sidebar — the functional-parity first cut of
// cmux's `CmuxSidebar` / sessions index. Renders the first window's live
// workspace list (from the Rust session snapshot) as selectable rows, with a
// "new workspace" (+) control and a per-row close (✕) on hover.
//
// Split into a presentational {@link SidebarView} (pure, prop-driven) and a thin
// {@link Sidebar} container that feeds it live data + mutations from
// `useSession`. Unlike the richer `WorkspaceList` (a dumb renderer of
// already-projected `SidebarWorkspaceRenderItem`s), `SidebarView` projects the
// row title + close affordance inline over the raw snapshot; the two converge
// once groups/pins reach the live model. Every `useSession` instance stays in
// sync through the broadcast `cmux://session-changed` event, so this sidebar and
// the main `Workspace` reconcile to the same snapshot after any change.

import { Icon } from "@cmux/webviews/src/icons";

import { useSession } from "../hooks/useSession";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

/** The label shown for a workspace row: its custom title, else process title. */
function workspaceTitle(ws: SessionWorkspaceSnapshot): string {
  return ws.custom_title?.trim() || ws.process_title?.trim() || "Terminal";
}

export interface SidebarViewProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
  /** The first window's workspaces, in order. */
  workspaces: readonly SessionWorkspaceSnapshot[];
  /** Index of the selected workspace in `workspaces`. */
  selectedWorkspaceIndex: number;
  onNewWorkspace: () => void;
  onSelectWorkspace: (index: number) => void;
  onCloseWorkspace: (index: number) => void;
}

/** Pure, prop-driven sidebar list — no data source, so it renders headlessly. */
export function SidebarView({
  collapsed,
  workspaces,
  selectedWorkspaceIndex,
  onNewWorkspace,
  onSelectWorkspace,
  onCloseWorkspace,
}: SidebarViewProps): React.JSX.Element {
  if (collapsed) {
    return <div className="cmux-sidebar cmux-sidebar--collapsed" aria-hidden="true" />;
  }

  // Canonical `TabManager.closeWorkspace` is a no-op when `tabs.count <= 1`, so
  // the sole remaining workspace has no close affordance (its ✕ is hidden).
  const canClose = workspaces.length > 1;

  return (
    <nav className="cmux-sidebar" aria-label="Workspaces">
      <div className="cmux-sidebar-header">
        <span className="cmux-sidebar-title">Workspaces</span>
        <button
          type="button"
          className="cmux-sidebar-new"
          title="New workspace"
          aria-label="New workspace"
          onClick={() => onNewWorkspace()}
        >
          +
        </button>
      </div>
      <ul className="cmux-sidebar-list">
        {workspaces.map((ws, index) => {
          const selected = index === selectedWorkspaceIndex;
          const title = workspaceTitle(ws);
          return (
            <li
              key={ws.workspace_id ?? `ws-${index}`}
              className={selected ? "cmux-sidebar-row is-selected" : "cmux-sidebar-row"}
              aria-selected={selected}
              onClick={() => onSelectWorkspace(index)}
            >
              <span className="cmux-icon cmux-sidebar-row-icon" aria-hidden="true">
                <Icon name="classic" />
              </span>
              <span className="cmux-sidebar-row-label">{title}</span>
              {canClose ? (
                <button
                  type="button"
                  className="cmux-sidebar-row-close"
                  title="Close workspace"
                  aria-label={`Close ${title}`}
                  onClick={(event) => {
                    // Don't let the row's select handler fire on close.
                    event.stopPropagation();
                    onCloseWorkspace(index);
                  }}
                >
                  ✕
                </button>
              ) : null}
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

export interface SidebarProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
}

/** Live container: binds {@link SidebarView} to the `useSession` snapshot. */
export function Sidebar({ collapsed }: SidebarProps): React.JSX.Element {
  const {
    workspaces,
    selectedWorkspaceIndex,
    newWorkspace,
    selectWorkspace,
    closeWorkspace,
  } = useSession();

  return (
    <SidebarView
      collapsed={collapsed}
      workspaces={workspaces}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={newWorkspace}
      onSelectWorkspace={selectWorkspace}
      onCloseWorkspace={closeWorkspace}
    />
  );
}
