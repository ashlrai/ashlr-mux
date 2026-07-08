// The left sessions/workspace sidebar — canonical cmux chrome parity. Renders
// the first window's live workspace list (from the Rust session snapshot)
// through the ported `renderItems` projection, so groups (headers, collapse,
// member counts), pins, and selection all draw exactly as the canonical
// sidebar orders them, with a "new workspace" (+) control and a per-row close
// (✕) on hover.
//
// Split into a presentational {@link SidebarView} (pure, prop-driven: projects
// the snapshot slice via `projectSidebarItems` and renders `WorkspaceList`) and
// a thin {@link Sidebar} container that feeds it live data + mutations from
// `useSession`. Every `useSession` instance stays in sync through the broadcast
// `cmux://session-changed` event, so this sidebar and the main `Workspace`
// reconcile to the same snapshot after any change.
//
// The session commands address workspaces by index, while render items carry
// stable workspace ids — the id → index translation happens here, over the same
// normalized id keys the projection uses. Rows whose snapshot lacks a
// `workspace_id` are skipped by the projection (parity with the Rust
// `sidebar_render`); the live session layer mints ids for every workspace
// (`ensure_workspace_ids`), so this only drops corrupted rows.

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { useSession } from "../hooks/useSession";
import {
  projectSidebarItems,
  workspaceIdKey,
  workspaceTitlesById,
} from "../sidebar/snapshotProjection";
import { WorkspaceList } from "./WorkspaceList";

export interface SidebarViewProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
  /** The first window's workspaces, in persisted (already pin/group) order. */
  workspaces: readonly SessionWorkspaceSnapshot[];
  /** The first window's workspace groups, if any. */
  workspaceGroups?: readonly SessionWorkspaceGroupSnapshot[];
  /** Index of the selected workspace in `workspaces`. */
  selectedWorkspaceIndex: number;
  onNewWorkspace: () => void;
  onSelectWorkspace: (index: number) => void;
  onCloseWorkspace: (index: number) => void;
  /**
   * Inline-rename commit for the workspace at `index`. Empty/whitespace-only
   * clears the custom title (canonical `setCustomTitle` parity — the display
   * falls back to the process title).
   */
  onRenameWorkspace: (index: number, title: string) => void;
  /**
   * Pin/unpin the workspace at `index`. Pinned rows normalize to the top
   * tier Rust-side (canonical `setPinned` pinned-ahead reorder).
   */
  onSetWorkspacePinned: (index: number, pinned: boolean) => void;
  /**
   * Chevron activation — sets the group's collapsed state. Groups are
   * addressed by stable id (no id → index translation; that map is for
   * workspace ids only).
   */
  onToggleGroupCollapsed: (groupId: string, nextCollapsed: boolean) => void;
}

/** Pure, prop-driven sidebar — no data source, so it renders headlessly. */
export function SidebarView({
  collapsed,
  workspaces,
  workspaceGroups,
  selectedWorkspaceIndex,
  onNewWorkspace,
  onSelectWorkspace,
  onCloseWorkspace,
  onRenameWorkspace,
  onSetWorkspacePinned,
  onToggleGroupCollapsed,
}: SidebarViewProps): React.JSX.Element {
  if (collapsed) {
    return <div className="cmux-sidebar cmux-sidebar--collapsed" aria-hidden="true" />;
  }

  const items = projectSidebarItems({
    workspaces,
    workspace_groups: workspaceGroups,
  });
  const titles = workspaceTitlesById(workspaces);

  // id → index over the same normalized keys the projection emits (keep-first
  // on duplicates, matching the projection's duplicate policy).
  const indexByWorkspaceId = new Map<string, number>();
  workspaces.forEach((ws, index) => {
    const id = workspaceIdKey(ws.workspace_id);
    if (id !== undefined && !indexByWorkspaceId.has(id)) {
      indexByWorkspaceId.set(id, index);
    }
  });

  const selectedId = workspaceIdKey(
    workspaces[selectedWorkspaceIndex]?.workspace_id,
  );
  const selectedWorkspaceIds = new Set<string>(
    selectedId !== undefined ? [selectedId] : [],
  );

  // Translate an id-addressed row action into the index-addressed session
  // command, forwarding any extra args. Stale/unknown id ⇒ silent no-op.
  const withIndexOf =
    <A extends unknown[]>(action: (index: number, ...rest: A) => void) =>
    (workspaceId: string, ...rest: A) => {
      const index = indexByWorkspaceId.get(workspaceId);
      if (index !== undefined) {
        action(index, ...rest);
      }
    };

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
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={selectedWorkspaceIds}
        titleForWorkspace={(id) => titles.get(id) ?? "Terminal"}
        canCloseWorkspaces={canClose}
        onSelectWorkspace={withIndexOf(onSelectWorkspace)}
        onCloseWorkspace={withIndexOf(onCloseWorkspace)}
        onRenameWorkspace={withIndexOf(onRenameWorkspace)}
        onSetWorkspacePinned={withIndexOf(onSetWorkspacePinned)}
        onToggleGroupCollapsed={onToggleGroupCollapsed}
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
    workspaces,
    workspaceGroups,
    selectedWorkspaceIndex,
    newWorkspace,
    selectWorkspace,
    closeWorkspace,
    renameWorkspace,
    setWorkspacePinned,
    setGroupCollapsed,
  } = useSession();

  return (
    <SidebarView
      collapsed={collapsed}
      workspaces={workspaces}
      workspaceGroups={workspaceGroups}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={newWorkspace}
      onSelectWorkspace={selectWorkspace}
      onCloseWorkspace={closeWorkspace}
      onRenameWorkspace={renameWorkspace}
      onSetWorkspacePinned={setWorkspacePinned}
      onToggleGroupCollapsed={setGroupCollapsed}
    />
  );
}
