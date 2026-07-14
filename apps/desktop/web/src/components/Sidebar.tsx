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

import { useEffect, useRef, useState, type ReactNode } from "react";

import type {
  BranchLayout,
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";
import { Icon } from "@cmux/webviews/src/icons";

import { host } from "../host/host";
import { useSession } from "../hooks/useSession";
import type { BadgeVisibilitySettings, WorkspaceBadges } from "../sidebar/badges";
import type { WorkspaceContextMenuAction } from "../sidebar/contextMenu";
import { badgesForSessionWorkspace } from "../sidebar/sessionBadges";
import {
  planSidebarWorkspaceDrop,
  type SidebarDropEdge,
} from "../sidebar/dragReorder";
import {
  anchorIndexAfterWorkspaceReorder,
  anchorWorkspaceId,
  reconciledSelection,
  selectionAfterWorkspaceClick,
  type WorkspaceClickModifiers,
} from "../sidebar/selection";
import {
  projectSidebarItems,
  workspaceDescriptionsById,
  workspaceIdKey,
  workspaceTitlesById,
} from "../sidebar/snapshotProjection";
import {
  WorkspaceList,
  type WorkspaceProgress,
  type WorkspaceSidebarDetails,
} from "./WorkspaceList";
import { CustomSidebarSurface } from "./CustomSidebarSurface";

const CUSTOM_SIDEBAR_SELECT_EVENT = "cmux://custom-sidebar-select";

interface CustomSidebarOption {
  name: string;
  kind: string;
  path: string;
  valid?: boolean;
  manifest_path?: string;
  manifest?: CustomSidebarManifestSummary | null;
}

interface CustomSidebarSelection {
  name: string;
  kind: string;
  path: string;
  sidebar?: {
    manifest?: CustomSidebarManifestSummary | null;
  };
  manifest?: CustomSidebarManifestSummary | null;
}

interface CustomSidebarValidationPayload {
  sidebars?: CustomSidebarOption[];
}

interface CustomSidebarManifestSummary {
  path?: string;
  valid?: boolean;
  trusted?: boolean;
  requested_methods?: string[];
  allowed_requested_methods?: string[];
  denied_requested_methods?: string[];
  errors?: string[];
  enforced?: boolean;
}

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

function unreadWorkspaceIdKeys(
  workspaces: readonly SessionWorkspaceSnapshot[],
): ReadonlySet<string> {
  const ids = new Set<string>();
  for (const workspace of workspaces) {
    const id = workspaceIdKey(workspace.workspace_id);
    if (
      id !== undefined &&
      workspace.panel_unreads?.some((entry) => entry.is_unread) === true
    ) {
      ids.add(id);
    }
  }
  return ids;
}

function workspaceBadgesById(
  workspaces: readonly SessionWorkspaceSnapshot[],
  settings?: Partial<BadgeVisibilitySettings>,
): Map<string, WorkspaceBadges> {
  const badges = new Map<string, WorkspaceBadges>();
  for (const workspace of workspaces) {
    const id = workspaceIdKey(workspace.workspace_id);
    if (id !== undefined && !badges.has(id)) {
      badges.set(id, badgesForSessionWorkspace(workspace, settings));
    }
  }
  return badges;
}

function workspaceProgressById(
  workspaces: readonly SessionWorkspaceSnapshot[],
  visible: boolean,
): Map<string, WorkspaceProgress> {
  const progress = new Map<string, WorkspaceProgress>();
  if (!visible) {
    return progress;
  }
  for (const workspace of workspaces) {
    const id = workspaceIdKey(workspace.workspace_id);
    const snapshotProgress = workspace.sidebar_progress;
    if (id !== undefined && snapshotProgress !== undefined && !progress.has(id)) {
      progress.set(id, {
        value: snapshotProgress.value,
        label: snapshotProgress.label,
      });
    }
  }
  return progress;
}

function workspaceSidebarDetailsById(
  workspaces: readonly SessionWorkspaceSnapshot[],
  options: {
    showStatus: boolean;
    showLog: boolean;
  },
): Map<string, WorkspaceSidebarDetails> {
  const details = new Map<string, WorkspaceSidebarDetails>();
  if (!options.showStatus && !options.showLog) {
    return details;
  }
  for (const workspace of workspaces) {
    const id = workspaceIdKey(workspace.workspace_id);
    if (id === undefined || details.has(id)) {
      continue;
    }
    const metadataEntries = options.showStatus
      ? (workspace.sidebar_metadata_entries ?? []).map((entry) => ({
          key: entry.key,
          value: entry.value,
          icon: entry.icon,
          color: entry.color,
          url: entry.url,
          priority: entry.priority,
          format: entry.format,
        }))
      : [];
    const metadataKeys = new Set(metadataEntries.map((entry) => entry.key));
    const statusEntries = options.showStatus
      ? (workspace.sidebar_status_entries ?? []).map((entry) => ({
          key: entry.key,
          value: entry.value,
          priority: entry.priority,
        })).filter((entry) => !metadataKeys.has(entry.key))
      : [];
    const metadataBlocks = options.showStatus
      ? (workspace.sidebar_metadata_blocks ?? []).map((entry) => ({
          key: entry.key,
          markdown: entry.markdown,
          priority: entry.priority,
        }))
      : [];
    const logEntries = options.showLog
      ? [...(workspace.sidebar_log_entries ?? [])]
          .slice(-2)
          .reverse()
          .map((entry) => ({
            level: entry.level,
            message: entry.message,
            createdAt: entry.created_at,
          }))
      : [];
    if (
      statusEntries.length > 0 ||
      metadataEntries.length > 0 ||
      metadataBlocks.length > 0 ||
      logEntries.length > 0
    ) {
      details.set(id, {
        statusEntries,
        metadataEntries,
        metadataBlocks,
        logEntries,
      });
    }
  }
  return details;
}

export interface SidebarViewProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
  /** Whether custom workspace descriptions should be visible. */
  showWorkspaceDescription?: boolean;
  /** Whether workspace titles can wrap instead of always ellipsizing. */
  wrapWorkspaceTitles?: boolean;
  /** Branch display mode: vertical per-branch badges or compact inline summary. */
  branchLayout?: BranchLayout;
  /** Sidebar detail visibility settings that gate branch/PR/SSH/port badges. */
  badgeVisibilitySettings?: Partial<BadgeVisibilitySettings>;
  /** Whether workspace progress rows should render. */
  showProgress?: boolean;
  /** Whether sidebar log rows should render. */
  showLog?: boolean;
  /** Whether custom sidebar status/metadata rows should render. */
  showCustomMetadata?: boolean;
  makePullRequestsClickable?: boolean;
  openPullRequestLinksInCmuxBrowser?: boolean;
  openPortLinksInCmuxBrowser?: boolean;
  /** The first window's workspaces, in persisted (already pin/group) order. */
  workspaces: readonly SessionWorkspaceSnapshot[];
  /** The first window's workspace groups, if any. */
  workspaceGroups?: readonly SessionWorkspaceGroupSnapshot[];
  /** Index of the selected workspace in `workspaces`. */
  selectedWorkspaceIndex: number;
  onNewWorkspace: () => void;
  onNewBrowserWorkspace?: (url?: string) => void;
  onOpenWorkspaceFolder?: () => void;
  onOpenWorkspacePullRequests?: () => void;
  onSelectWorkspace: (index: number) => void;
  onCloseWorkspace: (index: number) => void;
  /** Close several workspaces addressed by their original session indices. */
  onCloseWorkspaces?: (indices: readonly number[]) => void;
  /**
   * Inline-rename commit for the workspace at `index`. Empty/whitespace-only
   * clears the custom title (canonical `setCustomTitle` parity — the display
   * falls back to the process title).
   */
  onRenameWorkspace: (index: number, title: string) => void;
  /**
   * Reorder the workspace at raw `index` toward `toIndex`. Sidebar drags can
   * opt into top-level row space for group anchors and grouped-child promotion.
   */
  onReorderWorkspace: (
    index: number,
    toIndex: number,
    usesTopLevelRows?: boolean,
  ) => void;
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
  /** Optional git/PR badge descriptors keyed by workspace id. */
  badgesForWorkspace?: (workspaceId: string) => WorkspaceBadges | undefined;
  /** Discovered custom sidebars that can replace the workspace list. */
  customSidebars?: readonly CustomSidebarOption[];
  /** The currently selected custom sidebar for the left sidebar host. */
  selectedCustomSidebar?: CustomSidebarSelection | null;
  customSidebarError?: string | null;
  customSidebarContent?: ReactNode;
  onSelectCustomSidebar?: (name: string) => void;
  onShowWorkspaceSidebar?: () => void;
  onReloadCustomSidebar?: (name?: string) => void;
}

/** Pure, prop-driven sidebar — no data source, so it renders headlessly. */
export function SidebarView({
  collapsed,
  showWorkspaceDescription = false,
  wrapWorkspaceTitles = false,
  branchLayout = "vertical",
  badgeVisibilitySettings,
  showProgress = true,
  showLog = true,
  showCustomMetadata = true,
  makePullRequestsClickable = true,
  openPullRequestLinksInCmuxBrowser = false,
  openPortLinksInCmuxBrowser = false,
  workspaces,
  workspaceGroups,
  selectedWorkspaceIndex,
  onNewWorkspace,
  onNewBrowserWorkspace,
  onOpenWorkspaceFolder,
  onOpenWorkspacePullRequests,
  onSelectWorkspace,
  onCloseWorkspace,
  onCloseWorkspaces,
  onRenameWorkspace,
  onReorderWorkspace,
  onSetWorkspacePinned,
  onToggleGroupCollapsed,
  multiSelectedWorkspaceIds,
  selectionAnchorIndex,
  onMultiSelectionChange,
  badgesForWorkspace,
  customSidebars = [],
  selectedCustomSidebar = null,
  customSidebarError = null,
  customSidebarContent,
  onSelectCustomSidebar,
  onShowWorkspaceSidebar,
  onReloadCustomSidebar,
}: SidebarViewProps): React.JSX.Element {
  if (collapsed) {
    return <div className="cmux-sidebar cmux-sidebar--collapsed" aria-hidden="true" />;
  }

  const selectedCustomSidebarOption =
    selectedCustomSidebar === null
      ? undefined
      : customSidebars.find(
          (sidebar) =>
            sidebar.name === selectedCustomSidebar.name ||
            sidebar.path === selectedCustomSidebar.path,
        );
  const selectedCustomSidebarManifest =
    selectedCustomSidebarOption?.manifest ??
    selectedCustomSidebar?.manifest ??
    selectedCustomSidebar?.sidebar?.manifest ??
    null;

  const customSidebarPicker =
    customSidebars.length > 0 ? (
      <select
        className="cmux-sidebar-custom-picker"
        aria-label="Select custom sidebar"
        value={selectedCustomSidebar?.name ?? ""}
        onChange={(event) => {
          const name = event.currentTarget.value;
          if (name) {
            onSelectCustomSidebar?.(name);
          }
        }}
      >
        <option value="">Sidebars</option>
        {customSidebars.map((sidebar) => (
          <option key={`${sidebar.kind}:${sidebar.name}`} value={sidebar.name}>
            {sidebar.name}
          </option>
        ))}
      </select>
    ) : null;

  if (selectedCustomSidebar !== null) {
    return (
      <nav className="cmux-sidebar cmux-sidebar--custom" aria-label="Custom sidebar">
        <div className="cmux-sidebar-header">
          <button
            type="button"
            className="cmux-sidebar-folder"
            title="Show workspaces"
            aria-label="Show workspaces"
            onClick={() => onShowWorkspaceSidebar?.()}
          >
            Back
          </button>
          <span className="cmux-sidebar-title">{selectedCustomSidebar.name}</span>
          <div className="cmux-sidebar-header-actions">
            {customSidebarPicker}
            <button
              type="button"
              className="cmux-sidebar-folder"
              title="Reload custom sidebar"
              aria-label="Reload custom sidebar"
              onClick={() => onReloadCustomSidebar?.(selectedCustomSidebar.name)}
            >
              Reload
            </button>
          </div>
        </div>
        {customSidebarError ? (
          <div className="cmux-sidebar-custom-error" role="alert">
            {customSidebarError}
          </div>
        ) : null}
        <CustomSidebarManifestStrip manifest={selectedCustomSidebarManifest} />
        <div className="cmux-sidebar-custom-host">
          {customSidebarContent ?? (
            <CustomSidebarSurface sourcePath={selectedCustomSidebar.path} />
          )}
        </div>
      </nav>
    );
  }

  const items = projectSidebarItems({
    workspaces,
    workspace_groups: workspaceGroups,
  });
  const titles = workspaceTitlesById(workspaces);
  const descriptions = showWorkspaceDescription
    ? workspaceDescriptionsById(workspaces)
    : new Map<string, string>();
  const unreadWorkspaceIds = unreadWorkspaceIdKeys(workspaces);
  const sessionBadgesById = workspaceBadgesById(workspaces, badgeVisibilitySettings);
  const progressById = workspaceProgressById(
    workspaces,
    showProgress && badgeVisibilitySettings?.hideAllDetails !== true,
  );
  const detailsById = workspaceSidebarDetailsById(workspaces, {
    showStatus: showCustomMetadata && badgeVisibilitySettings?.hideAllDetails !== true,
    showLog: showLog && badgeVisibilitySettings?.hideAllDetails !== true,
  });

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

  const handleWorkspaceReorder = (
    draggedWorkspaceId: string,
    targetWorkspaceId: string,
    edge: SidebarDropEdge,
  ) => {
    const plan = planSidebarWorkspaceDrop({
      draggedWorkspaceId,
      targetWorkspaceId,
      edge,
      workspaces,
      items,
    });
    if (plan != null) {
      onReorderWorkspace(plan.index, plan.toIndex, plan.usesTopLevelRows);
    }
  };

  // Canonical `TabManager.closeWorkspace` is a no-op when `tabs.count <= 1`, so
  // the sole remaining workspace has no close affordance (its ✕ is hidden).
  const canClose = workspaces.length > 1;
  const indicesForWorkspaceIds = (workspaceIds: readonly string[]): number[] => {
    const indices: number[] = [];
    for (const workspaceId of workspaceIds) {
      const index = indexByWorkspaceId.get(workspaceId);
      if (index !== undefined) {
        indices.push(index);
      }
    }
    return indices;
  };
  const handleContextMenuAction = (action: WorkspaceContextMenuAction) => {
    switch (action.kind) {
      case "newWorkspace":
        onNewWorkspace();
        return;
      case "setWorkspacePinned":
        withIndexOf(onSetWorkspacePinned)(action.workspaceId, action.pinned);
        return;
      case "closeWorkspace":
        withIndexOf(onCloseWorkspace)(action.workspaceId);
        return;
      case "closeOtherWorkspaces": {
        const keepIndex = indexByWorkspaceId.get(action.workspaceId);
        if (keepIndex === undefined || onCloseWorkspaces === undefined) {
          return;
        }
        onCloseWorkspaces(
          workspaces.map((_, index) => index).filter((index) => index !== keepIndex),
        );
        return;
      }
      case "setGroupCollapsed":
        onToggleGroupCollapsed(action.groupId, action.collapsed);
        return;
      case "closeGroupWorkspaces":
        onCloseWorkspaces?.(indicesForWorkspaceIds(action.workspaceIds));
        return;
      case "beginRenameWorkspace":
        return;
    }
  };

  return (
    <nav className="cmux-sidebar" aria-label="Workspaces">
      <div className="cmux-sidebar-header">
        <span className="cmux-sidebar-title">Workspaces</span>
        <div className="cmux-sidebar-header-actions">
          {customSidebarPicker}
          <button
            type="button"
            className="cmux-sidebar-folder"
            title="Open workspace pull requests"
            aria-label="Open workspace pull requests"
            disabled={onOpenWorkspacePullRequests == null}
            onClick={() => onOpenWorkspacePullRequests?.()}
          >
            PRs
          </button>
          <button
            type="button"
            className="cmux-sidebar-folder"
            title="Open folder as workspace"
            aria-label="Open folder as workspace"
            disabled={onOpenWorkspaceFolder == null}
            onClick={() => onOpenWorkspaceFolder?.()}
          >
            Open
          </button>
          <button
            type="button"
            className="cmux-sidebar-browser"
            title="New browser workspace"
            aria-label="New browser workspace"
            disabled={onNewBrowserWorkspace == null}
            onClick={() => onNewBrowserWorkspace?.()}
          >
            <Icon name="browser" />
          </button>
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
      </div>
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={selectedWorkspaceIds}
        multiSelectedWorkspaceIds={multiSelectedWorkspaceIds}
        unreadWorkspaceIds={unreadWorkspaceIds}
        titleForWorkspace={(id) => titles.get(id) ?? "Terminal"}
        wrapWorkspaceTitles={wrapWorkspaceTitles}
        descriptionForWorkspace={(id) => descriptions.get(id)}
        badgesForWorkspace={(id) => badgesForWorkspace?.(id) ?? sessionBadgesById.get(id)}
        progressForWorkspace={(id) => progressById.get(id)}
        detailsForWorkspace={(id) => detailsById.get(id)}
        branchLayout={branchLayout}
        makePullRequestsClickable={makePullRequestsClickable}
        openPullRequestLinksInCmuxBrowser={openPullRequestLinksInCmuxBrowser}
        openPortLinksInCmuxBrowser={openPortLinksInCmuxBrowser}
        onOpenUrlInCmuxBrowser={(url) => onNewBrowserWorkspace?.(url)}
        canCloseWorkspaces={canClose}
        onSelectWorkspace={handleWorkspaceClick}
        onCloseWorkspace={withIndexOf(onCloseWorkspace)}
        onRenameWorkspace={withIndexOf(onRenameWorkspace)}
        onReorderWorkspace={handleWorkspaceReorder}
        onSetWorkspacePinned={withIndexOf(onSetWorkspacePinned)}
        onToggleGroupCollapsed={onToggleGroupCollapsed}
        onContextMenuAction={handleContextMenuAction}
      />
    </nav>
  );
}

export interface SidebarProps {
  /** Whether the sidebar is collapsed to a rail (hidden list). */
  collapsed: boolean;
  /** Whether custom workspace descriptions should be visible. */
  showWorkspaceDescription?: boolean;
  /** Whether workspace titles can wrap instead of always ellipsizing. */
  wrapWorkspaceTitles?: boolean;
  /** Branch display mode: vertical per-branch badges or compact inline summary. */
  branchLayout?: BranchLayout;
  /** Sidebar detail visibility settings that gate branch/PR/SSH/port badges. */
  badgeVisibilitySettings?: Partial<BadgeVisibilitySettings>;
  showProgress?: boolean;
  showLog?: boolean;
  showCustomMetadata?: boolean;
  makePullRequestsClickable?: boolean;
  openPullRequestLinksInCmuxBrowser?: boolean;
  openPortLinksInCmuxBrowser?: boolean;
}

function CustomSidebarManifestStrip({
  manifest,
}: {
  manifest?: CustomSidebarManifestSummary | null;
}): React.JSX.Element {
  const requested = manifest?.requested_methods ?? [];
  const allowed = manifest?.allowed_requested_methods ?? [];
  const denied = manifest?.denied_requested_methods ?? [];
  if (manifest == null) {
    return (
      <div className="cmux-sidebar-custom-manifest cmux-sidebar-custom-manifest-muted">
        <span className="cmux-sidebar-custom-manifest-status">Safe default policy</span>
        <span>No capability manifest</span>
      </div>
    );
  }
  const invalid = manifest.valid === false;
  const hasDenied = denied.length > 0;
  const status = invalid
    ? "Invalid manifest"
    : hasDenied
      ? "Some requests denied"
      : manifest.trusted
        ? "Trusted manifest"
        : "Manifest declared";
  return (
    <div
      className={`cmux-sidebar-custom-manifest${
        invalid || hasDenied ? " cmux-sidebar-custom-manifest-warn" : ""
      }`}
    >
      <span className="cmux-sidebar-custom-manifest-status">{status}</span>
      {requested.length > 0 ? (
        <span>{requested.length} requested</span>
      ) : (
        <span>No extra methods requested</span>
      )}
      {allowed.length > 0 ? (
        <span className="cmux-sidebar-custom-manifest-allowed">
          {allowed.length} allowed
        </span>
      ) : null}
      {denied.length > 0 ? (
        <span className="cmux-sidebar-custom-manifest-denied">
          Denied: {denied.slice(0, 2).join(", ")}
          {denied.length > 2 ? ` +${denied.length - 2}` : ""}
        </span>
      ) : null}
      {invalid && manifest.errors !== undefined && manifest.errors.length > 0 ? (
        <span className="cmux-sidebar-custom-manifest-denied">
          {manifest.errors[0]}
        </span>
      ) : null}
    </div>
  );
}

/** Live container: binds {@link SidebarView} to the `useSession` snapshot. */
export function Sidebar({
  collapsed,
  showWorkspaceDescription = false,
  wrapWorkspaceTitles = false,
  branchLayout = "vertical",
  badgeVisibilitySettings,
  showProgress = true,
  showLog = true,
  showCustomMetadata = true,
  makePullRequestsClickable = true,
  openPullRequestLinksInCmuxBrowser = false,
  openPortLinksInCmuxBrowser = false,
}: SidebarProps): React.JSX.Element {
  const {
    snapshot,
    workspaces,
    workspaceGroups,
    selectedWorkspaceIndex,
    newWorkspace,
    newBrowserWorkspace,
    selectWorkspace,
    closeWorkspace,
    closeWorkspaces,
    reorderWorkspace,
    renameWorkspace,
    setWorkspacePinned,
    setGroupCollapsed,
  } = useSession();

  const openWorkspaceFolder = () => {
    void host
      .invoke<string | null>("pick_workspace_folder")
      .then((currentDirectory) => {
        if (currentDirectory) {
          newWorkspace(currentDirectory);
        }
      })
      .catch((error) => {
        console.error("pick_workspace_folder failed", error);
      });
  };

  const openWorkspacePullRequests = () => {
    const directory = workspaces[selectedWorkspaceIndex]?.current_directory ?? "";
    void host
      .invoke<string[]>("workspace_pull_request_links", { directory })
      .then((urls) => {
        for (const url of urls) {
          newBrowserWorkspace(url);
        }
      })
      .catch((error) => {
        console.error("workspace_pull_request_links failed", error);
      });
  };

  const [customSidebars, setCustomSidebars] = useState<CustomSidebarOption[]>([]);
  const [selectedCustomSidebar, setSelectedCustomSidebar] =
    useState<CustomSidebarSelection | null>(null);
  const [customSidebarError, setCustomSidebarError] = useState<string | null>(null);

  const loadCustomSidebars = () => {
    void host
      .invoke<CustomSidebarValidationPayload>("custom_sidebar_action_invoke", {
        method: "sidebar.list",
        params: {},
      })
      .then((payload) => {
        setCustomSidebars(
          (payload.sidebars ?? [])
            .filter((sidebar) => sidebar.valid !== false && sidebar.path?.trim())
            .map((sidebar) => ({
              name: sidebar.name,
              kind: sidebar.kind,
              path: sidebar.path,
              valid: sidebar.valid,
              manifest_path: sidebar.manifest_path,
              manifest: sidebar.manifest,
            })),
        );
        setCustomSidebarError(null);
      })
      .catch((error) => {
        const message = error instanceof Error ? error.message : String(error);
        if (!message.includes("Tauri bridge is unavailable")) {
          setCustomSidebarError(message);
        }
      });
  };

  const selectCustomSidebar = (name: string) => {
    void host
      .invoke<CustomSidebarSelection>("custom_sidebar_action_invoke", {
        method: "sidebar.select",
        params: { name },
      })
      .then((selection) => {
        setSelectedCustomSidebar(selection);
        setCustomSidebarError(null);
        loadCustomSidebars();
      })
      .catch((error) => {
        setCustomSidebarError(error instanceof Error ? error.message : String(error));
      });
  };

  const reloadCustomSidebar = (name?: string) => {
    void host
      .invoke("custom_sidebar_action_invoke", {
        method: "sidebar.reload",
        params: name ? { name } : {},
      })
      .then(() => {
        setCustomSidebarError(null);
      })
      .catch((error) => {
        setCustomSidebarError(error instanceof Error ? error.message : String(error));
      });
  };

  useEffect(() => {
    loadCustomSidebars();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void host
      .on<CustomSidebarSelection>(CUSTOM_SIDEBAR_SELECT_EVENT, (selection) => {
        if (!disposed) {
          setSelectedCustomSidebar(selection);
          setCustomSidebarError(null);
          loadCustomSidebars();
        }
      })
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((error) => {
        console.error("custom sidebar select listener failed", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Multi-selection is VIEW state, not session state — the canonical @State
  // pair `selectedTabIds` + `lastSidebarSelectionIndex` (ContentView:984/986).
  const [multiSelection, setMultiSelection] = useState<ReadonlySet<string>>(
    EMPTY_SELECTION,
  );
  const [anchorIndex, setAnchorIndex] = useState<number | undefined>(undefined);

  const selectionWindowId = snapshot?.windows[0]?.window_id;
  useEffect(() => {
    void host
      .invoke("sidebar_selection_update", {
        windowId: selectionWindowId ?? null,
        workspaceIds: [...multiSelection],
      })
      .catch((error) => {
        console.error("sidebar selection mirror failed", error);
      });
  }, [multiSelection, selectionWindowId]);

  const liveIds = liveWorkspaceIdKeys(workspaces);
  const selectedId = workspaceIdKey(
    workspaces[selectedWorkspaceIndex]?.workspace_id,
  );
  const gitRefreshKey = workspaces
    .map(
      (workspace) =>
        `${workspaceIdKey(workspace.workspace_id) ?? ""}:${workspace.current_directory ?? ""}`,
    )
    .join("\n");

  useEffect(() => {
    let disposed = false;
    const refresh = () => {
      if (disposed) {
        return;
      }
      void host.invoke("workspace_git_refresh").catch((error) => {
        console.error("workspace_git_refresh failed", error);
      });
    };
    const timerId = window.setTimeout(refresh, 300);
    const intervalId = window.setInterval(refresh, 60_000);
    return () => {
      disposed = true;
      window.clearTimeout(timerId);
      window.clearInterval(intervalId);
    };
  }, [gitRefreshKey]);

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
      showWorkspaceDescription={showWorkspaceDescription}
      wrapWorkspaceTitles={wrapWorkspaceTitles}
      branchLayout={branchLayout}
      badgeVisibilitySettings={badgeVisibilitySettings}
      showProgress={showProgress}
      showLog={showLog}
      showCustomMetadata={showCustomMetadata}
      makePullRequestsClickable={makePullRequestsClickable}
      openPullRequestLinksInCmuxBrowser={openPullRequestLinksInCmuxBrowser}
      openPortLinksInCmuxBrowser={openPortLinksInCmuxBrowser}
      workspaces={workspaces}
      workspaceGroups={workspaceGroups}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={newWorkspace}
      onNewBrowserWorkspace={newBrowserWorkspace}
      onOpenWorkspaceFolder={openWorkspaceFolder}
      onOpenWorkspacePullRequests={openWorkspacePullRequests}
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
      onCloseWorkspaces={closeWorkspaces}
      onReorderWorkspace={reorderWorkspace}
      onRenameWorkspace={renameWorkspace}
      onSetWorkspacePinned={setWorkspacePinned}
      onToggleGroupCollapsed={setGroupCollapsed}
      multiSelectedWorkspaceIds={multiSelection}
      selectionAnchorIndex={anchorIndex}
      onMultiSelectionChange={(ids, anchor) => {
        setMultiSelection(ids);
        setAnchorIndex(anchor);
      }}
      customSidebars={customSidebars}
      selectedCustomSidebar={selectedCustomSidebar}
      customSidebarError={customSidebarError}
      onSelectCustomSidebar={selectCustomSidebar}
      onShowWorkspaceSidebar={() => {
        setSelectedCustomSidebar(null);
        setCustomSidebarError(null);
      }}
      onReloadCustomSidebar={reloadCustomSidebar}
    />
  );
}
