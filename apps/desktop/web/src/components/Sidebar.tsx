// The left sessions/workspace sidebar — canonical cmux `CmuxSidebar` parity.
// Renders the first window's live workspace list through the golden-pinned
// projection pipeline: `SessionTabManagerSnapshot` → `snapshotRenderItems`
// (groups, pins, collapse, anchor suppression) → `WorkspaceList` rows.
//
// Split into a presentational {@link SidebarView} (pure, prop-driven over the
// tab-manager snapshot) and a thin {@link Sidebar} container that feeds it live
// data + mutations from `useSession`. Every `useSession` instance stays in sync
// through the broadcast `cmux://session-changed` event, so this sidebar and the
// main `Workspace` reconcile to the same snapshot after any change.
//
// The session commands address workspaces by INDEX into the snapshot's
// workspace array, while render items carry (normalized) workspace ids — the
// view translates id → index at dispatch time via a lookup built from the same
// normalization the projection uses.

import type {
  SessionTabManagerSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { useSession } from "../hooks/useSession";
import {
  normalizedWorkspaceId,
  snapshotRenderItems,
} from "../sidebar/liveRenderItems";
import { WorkspaceList } from "./WorkspaceList";

/** The label shown for a workspace row: its custom title, else process title. */
function workspaceTitle(ws: SessionWorkspaceSnapshot): string {
  return ws.custom_title?.trim() || ws.process_title?.trim() || "Terminal";
}

export interface SidebarViewProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
  /** The first window's tab manager, or `null` before the first snapshot. */
  tabs: SessionTabManagerSnapshot | null;
  onNewWorkspace: () => void;
  onSelectWorkspace: (index: number) => void;
  onCloseWorkspace: (index: number) => void;
  onRenameWorkspace?: (index: number, title: string) => void;
  onSetWorkspacePinned?: (index: number, pinned: boolean) => void;
  onSetGroupCollapsed?: (groupId: string, collapsed: boolean) => void;
}

/** Pure, prop-driven sidebar — no data source, so it renders headlessly. */
export function SidebarView({
  collapsed,
  tabs,
  onNewWorkspace,
  onSelectWorkspace,
  onCloseWorkspace,
  onRenameWorkspace,
  onSetWorkspacePinned,
  onSetGroupCollapsed,
}: SidebarViewProps): React.JSX.Element {
  if (collapsed) {
    return <div className="cmux-sidebar cmux-sidebar--collapsed" aria-hidden="true" />;
  }

  const workspaces = tabs?.workspaces ?? [];
  const items = tabs ? snapshotRenderItems(tabs) : [];

  // Id-keyed view state, normalized exactly like the projection's row ids so
  // lookups can't miss on UUID case.
  const titlesById = new Map<string, string>();
  const indexById = new Map<string, number>();
  workspaces.forEach((ws, index) => {
    const id = normalizedWorkspaceId(ws.workspace_id);
    if (id !== undefined) {
      titlesById.set(id, workspaceTitle(ws));
      indexById.set(id, index);
    }
  });

  const rawIndex = tabs?.selected_workspace_index ?? 0;
  const selectedIndex =
    rawIndex >= 0 && rawIndex < workspaces.length ? rawIndex : 0;
  const selectedId = normalizedWorkspaceId(
    workspaces[selectedIndex]?.workspace_id,
  );
  const selectedWorkspaceIds = new Set<string>(
    selectedId !== undefined ? [selectedId] : [],
  );

  const dispatchByIndex =
    (handler: (index: number) => void) => (workspaceId: string) => {
      const index = indexById.get(workspaceId);
      if (index !== undefined) {
        handler(index);
      }
    };

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
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={selectedWorkspaceIds}
        titlesById={titlesById}
        // Canonical `TabManager.closeWorkspace` is a no-op when
        // `tabs.count <= 1`, so the sole workspace has no close affordance.
        canCloseWorkspaces={workspaces.length > 1}
        onSelectWorkspace={dispatchByIndex(onSelectWorkspace)}
        onCloseWorkspace={dispatchByIndex(onCloseWorkspace)}
        onRenameWorkspace={
          onRenameWorkspace
            ? (workspaceId, title) => {
                const index = indexById.get(workspaceId);
                if (index !== undefined) {
                  onRenameWorkspace(index, title);
                }
              }
            : undefined
        }
        onSetWorkspacePinned={
          onSetWorkspacePinned
            ? (workspaceId, pinned) => {
                const index = indexById.get(workspaceId);
                if (index !== undefined) {
                  onSetWorkspacePinned(index, pinned);
                }
              }
            : undefined
        }
        onSetGroupCollapsed={onSetGroupCollapsed}
      />
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
    snapshot,
    newWorkspace,
    selectWorkspace,
    closeWorkspace,
    renameWorkspace,
    setGroupCollapsed,
    setWorkspacePinned,
  } = useSession();

  return (
    <SidebarView
      collapsed={collapsed}
      tabs={snapshot?.windows[0]?.tab_manager ?? null}
      onNewWorkspace={newWorkspace}
      onSelectWorkspace={selectWorkspace}
      onCloseWorkspace={closeWorkspace}
      onRenameWorkspace={renameWorkspace}
      onSetWorkspacePinned={setWorkspacePinned}
      onSetGroupCollapsed={setGroupCollapsed}
    />
  );
}
