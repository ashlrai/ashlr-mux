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

import { useEffect, useRef, useState } from "react";

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { useSession } from "../hooks/useSession";
import {
  anchorIndexAfterWorkspaceReorder,
  anchorWorkspaceId,
  reconciledSelection,
  selectionAfterWorkspaceClick,
  type WorkspaceClickModifiers,
} from "../sidebar/selection";
import {
  projectSidebarItems,
  workspaceIdKey,
  workspaceTitlesById,
} from "../sidebar/snapshotProjection";
import { WorkspaceList } from "./WorkspaceList";

const EMPTY_SELECTION: ReadonlySet<string> = new Set();

/// Session-ordered, index-aligned workspace id keys for the selection policy.
/// Id-less rows get a placeholder that keeps indices aligned without colliding
/// with real id keys (`workspaceIdKey` only emits lowercased UUIDs, never a
/// leading space); such rows are unclickable/unselectable anyway.
function liveWorkspaceIdKeys(
  workspaces: readonly SessionWorkspaceSnapshot[],
): string[] {
  return workspaces.map(
    (ws, i) => workspaceIdKey(ws.workspace_id) ?? ` missing:${i}`,
  );
}

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
  /**
   * The sidebar multi-selection (view state, canonical `@State selectedTabIds`,
   * ContentView.swift:984). Rows in it that are not the active row draw
   * `is-multi-selected`.
   */
  multiSelectedWorkspaceIds?: ReadonlySet<string>;
  /** Shift-click anchor (canonical `lastSidebarSelectionIndex`, ContentView:986). */
  selectionAnchorIndex?: number;
  /**
   * New multi-selection + anchor after a row click (the pure
   * `selectionAfterWorkspaceClick` result). Fired BEFORE `onSelectWorkspace`;
   * activation always follows (ContentView.swift:14320).
   */
  onMultiSelectionChange?: (
    selectedWorkspaceIds: Set<string>,
    anchorIndex: number,
  ) => void;
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
  multiSelectedWorkspaceIds,
  selectionAnchorIndex,
  onMultiSelectionChange,
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

  // Range indices run over the RAW session order — collapsed members are
  // absent from `items` but still occupy live indices.
  const liveWorkspaceIds = liveWorkspaceIdKeys(workspaces);
  // Collapsed-group members other than their group's anchor are excluded from
  // shift ranges (ContentView.swift:14284-14299). Derived from the projected
  // items so the group's 3-tier anchor resolution is reused.
  const hiddenWorkspaceIds = new Set<string>();
  for (const item of items) {
    if (item.kind === "groupHeader" && item.group.isCollapsed) {
      for (const id of item.memberWorkspaceIds) {
        if (id !== item.group.anchorWorkspaceId) {
          hiddenWorkspaceIds.add(id);
        }
      }
    }
  }

  const handleWorkspaceClick = (
    workspaceId: string,
    modifiers: WorkspaceClickModifiers,
  ) => {
    const index = indexByWorkspaceId.get(workspaceId);
    if (index === undefined) {
      return;
    }
    const result = selectionAfterWorkspaceClick({
      clickedIndex: index,
      modifiers,
      existingAnchorIndex: selectionAnchorIndex,
      selectedWorkspaceIds: multiSelectedWorkspaceIds ?? EMPTY_SELECTION,
      focusedWorkspaceId: selectedId,
      liveWorkspaceIds,
      hiddenWorkspaceIds,
    });
    onMultiSelectionChange?.(result.selectedWorkspaceIds, result.anchorIndex);
    // Activation ALWAYS fires — canonical `selectTab` is unconditional after
    // the selection update (ContentView.swift:14320), so shift/ctrl clicks
    // activate the clicked workspace too.
    onSelectWorkspace(index);
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
        multiSelectedWorkspaceIds={multiSelectedWorkspaceIds}
        titleForWorkspace={(id) => titles.get(id) ?? "Terminal"}
        canCloseWorkspaces={canClose}
        onSelectWorkspace={handleWorkspaceClick}
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

  // Multi-selection is VIEW state, not session state — the canonical @State
  // pair `selectedTabIds` + `lastSidebarSelectionIndex` (ContentView:984/986).
  const [multiSelection, setMultiSelection] = useState<ReadonlySet<string>>(
    EMPTY_SELECTION,
  );
  const [anchorIndex, setAnchorIndex] = useState<number | undefined>(undefined);

  const liveIds = liveWorkspaceIdKeys(workspaces);
  const selectedId = workspaceIdKey(
    workspaces[selectedWorkspaceIndex]?.workspace_id,
  );

  // Previous live ids, so a reorder can carry the anchor by id — canonical
  // captures the anchor's workspace id pre-reorder (ContentView:15722).
  const prevLiveIdsRef = useRef<readonly string[]>(liveIds);
  // True while a selected-workspace change was originated by a sidebar click
  // (whose reducer already produced the intended selection).
  const selectionChangeFromSidebarClick = useRef(false);
  const prevSelectedIdRef = useRef<string | undefined>(selectedId);

  // List-change maintenance (close/detach/restore/reorder — TabManager:2038,
  // 2077, 6049; ContentView:15912-15918). No fallback id is supplied to
  // `reconciledSelection`: closing every multi-selected row must EMPTY the
  // set (the fallback arm is the canonical restore/reorder path only).
  const liveKey = liveIds.join("\n");
  useEffect(() => {
    const prevLiveIds = prevLiveIdsRef.current;
    prevLiveIdsRef.current = liveIds;
    if (multiSelection.size === 0 && anchorIndex === undefined) {
      return; // Nothing to maintain.
    }
    const next = reconciledSelection(multiSelection, liveIds, undefined);
    const prevAnchorId = anchorWorkspaceId(anchorIndex, prevLiveIds);
    setMultiSelection(next);
    setAnchorIndex(
      anchorIndexAfterWorkspaceReorder(prevAnchorId, next, selectedId, liveIds),
    );
    // Runs only when the workspace-list identity changes; the closure reads
    // the state of that same render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [liveKey]);

  // Collapse on external navigation — parity with
  // `clearSidebarMultiSelection(except:)` for keyboard workspace nav
  // (TabManager.swift:3468/3484/3493-3498), which reaches
  // `useSession.selectWorkspace` outside this component.
  // DOCUMENTED DIVERGENCE: canonical collapses only on those two
  // keyboard-nav paths; the port collapses on ANY selected-workspace change
  // not originated by a sidebar click — a safe superset, because a canonical
  // plain click also collapses, and shift/ctrl clicks set the ref flag so
  // their own activation does not destroy the selection they just built.
  useEffect(() => {
    const prevSelectedId = prevSelectedIdRef.current;
    prevSelectedIdRef.current = selectedId;
    if (
      prevSelectedId !== selectedId &&
      !selectionChangeFromSidebarClick.current
    ) {
      setMultiSelection(
        selectedId !== undefined ? new Set([selectedId]) : EMPTY_SELECTION,
      );
      setAnchorIndex(selectedWorkspaceIndex);
    }
    // Always clear the flag once the activation it marked has landed.
    selectionChangeFromSidebarClick.current = false;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  return (
    <SidebarView
      collapsed={collapsed}
      workspaces={workspaces}
      workspaceGroups={workspaceGroups}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={newWorkspace}
      onSelectWorkspace={(index) => {
        // Mark the upcoming activation as sidebar-originated so the collapse
        // effect leaves the click's own selection intact. Skipped when the
        // clicked workspace is already active (no selected-id change would
        // ever clear the flag, and a stale flag would suppress a later
        // legitimate external-nav collapse).
        if (index !== selectedWorkspaceIndex) {
          selectionChangeFromSidebarClick.current = true;
        }
        selectWorkspace(index);
      }}
      onCloseWorkspace={closeWorkspace}
      onRenameWorkspace={renameWorkspace}
      onSetWorkspacePinned={setWorkspacePinned}
      onToggleGroupCollapsed={setGroupCollapsed}
      multiSelectedWorkspaceIds={multiSelection}
      selectionAnchorIndex={anchorIndex}
      onMultiSelectionChange={(ids, anchor) => {
        setMultiSelection(ids);
        setAnchorIndex(anchor);
      }}
    />
  );
}
