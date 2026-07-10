// Renders the projected `SidebarWorkspaceRenderItem[]` (see
// `../sidebar/renderItems`) into the sidebar: group headers (member count +
// collapse chevron) and workspace rows (pin state + selection + close).
// Pure/presentational — collapse suppression already happened in `renderItems`,
// so a collapsed group simply arrives here without its member rows.
//
// A group header IS its anchor workspace's row (renderItems suppresses the
// anchor as a plain row), so clicking a header selects the anchor. The chevron
// is a separate tap target that toggles collapse (stopPropagation, so it never
// selects the anchor) — canonical SidebarWorkspaceGroupHeaderView parity.
//
// Reuses the shared `@cmux/webviews` Icon for glyphs. The icon set has no
// dedicated folder/right-chevron, so a collapsed group shows the right-pointing
// `arrow` glyph and an expanded group the down-pointing `expand` glyph.

import {
  useCallback,
  useEffect,
  useState,
  type CSSProperties,
  type DragEvent,
} from "react";

import { Icon } from "@cmux/webviews/src/icons";

import type { SidebarWorkspaceRenderItem } from "../sidebar/renderItems";
import type {
  WorkspaceBadgeDescriptor,
  WorkspaceBadges,
} from "../sidebar/badges";
import {
  buildWorkspaceContextMenuItems,
  contextMenuKeyForTarget,
  contextMenuTargetMatches,
  type WorkspaceContextMenuAction,
  type WorkspaceContextMenuTarget,
  type WorkspaceContextMenuTargetKey,
} from "../sidebar/contextMenu";
import type { SidebarDropEdge } from "../sidebar/dragReorder";
import type { WorkspaceClickModifiers } from "../sidebar/selection";

export type SidebarBranchLayout = "vertical" | "inline";

export type WorkspaceProgress = {
  value: number;
  label?: string;
};

export type WorkspaceSidebarStatus = {
  key: string;
  value: string;
  priority?: number;
};

export type WorkspaceSidebarMetadata = {
  key: string;
  value: string;
  icon?: string;
  color?: string;
  url?: string;
  priority?: number;
  format?: string;
};

export type WorkspaceSidebarMetadataBlock = {
  key: string;
  markdown: string;
  priority?: number;
};

export type WorkspaceSidebarLogEntry = {
  level: string;
  message: string;
  createdAt: number;
};

export type WorkspaceSidebarDetails = {
  statusEntries: readonly WorkspaceSidebarStatus[];
  metadataEntries: readonly WorkspaceSidebarMetadata[];
  metadataBlocks: readonly WorkspaceSidebarMetadataBlock[];
  logEntries: readonly WorkspaceSidebarLogEntry[];
};

export interface WorkspaceListProps {
  /// The drawable items, already projected by `renderItems`.
  items: readonly SidebarWorkspaceRenderItem[];
  /// Ids of currently-selected workspaces. A group header counts as selected
  /// when its anchor workspace is selected.
  selectedWorkspaceIds?: ReadonlySet<string>;
  /// Ids in the sidebar multi-selection. A row draws `is-multi-selected` only
  /// when it is NOT the active row: canonical `sidebarWorkspaceRowBackgroundStyle`
  /// checks `isActive` before `isMultiSelected`
  /// (SidebarAppearanceSupport.swift:291-336), so the classes are exclusive.
  multiSelectedWorkspaceIds?: ReadonlySet<string>;
  /// Workspace ids carrying unread indicators.
  unreadWorkspaceIds?: ReadonlySet<string>;
  /// Resolves a workspace id to its row label; defaults to the id itself.
  titleForWorkspace?: (workspaceId: string) => string;
  /// Whether workspace titles can wrap instead of always ellipsizing.
  wrapWorkspaceTitles?: boolean;
  /// Optional secondary description text shown under the title.
  descriptionForWorkspace?: (workspaceId: string) => string | undefined;
  /// Optional git/PR badge descriptors for a workspace row. The producer owns
  /// canonical ordering/gates; this component only renders the descriptors.
  badgesForWorkspace?: (workspaceId: string) => WorkspaceBadges | undefined;
  /// Optional progress state rendered as a compact row meter.
  progressForWorkspace?: (workspaceId: string) => WorkspaceProgress | undefined;
  /// Optional sidebar status/log details rendered below badges/progress.
  detailsForWorkspace?: (workspaceId: string) => WorkspaceSidebarDetails | undefined;
  /// Branch display mode: vertical per-branch badges or compact inline summary.
  branchLayout?: SidebarBranchLayout;
  /// Whether PR badges render as links. When false, they render as inert pills.
  makePullRequestsClickable?: boolean;
  /// Route PR badge clicks into a cmux browser workspace instead of the href.
  openPullRequestLinksInCmuxBrowser?: boolean;
  /// Route port badge clicks into a cmux browser workspace instead of the href.
  openPortLinksInCmuxBrowser?: boolean;
  /// Opens a URL in a cmux browser workspace for link-routing settings.
  onOpenUrlInCmuxBrowser?: (url: string) => void;
  /// Whether workspace rows expose a close (✕) affordance. Canonical
  /// `TabManager.closeWorkspace` is a no-op with one workspace left, so the
  /// sole survivor hides it.
  canCloseWorkspaces?: boolean;
  /// Row activation — a group header activates its anchor workspace. Click
  /// modifiers ride along so the caller can extend the multi-selection;
  /// activation itself is unconditional (ContentView.swift:14320).
  onSelectWorkspace?: (
    workspaceId: string,
    modifiers: WorkspaceClickModifiers,
  ) => void;
  onReorderWorkspace?: (
    draggedWorkspaceId: string,
    targetWorkspaceId: string,
    edge: SidebarDropEdge,
  ) => void;
  onCloseWorkspace?: (workspaceId: string) => void;
  /// Row-level pin toggle. Rows expose the affordance only when provided
  /// (matches the rename gating). Group HEADER rows never get it: group pin is
  /// `setWorkspaceGroupPinned` (TabManager.swift:1846) — a different op.
  onSetWorkspacePinned?: (workspaceId: string, pinned: boolean) => void;
  /// Chevron activation — toggles the group's collapsed state.
  onToggleGroupCollapsed?: (groupId: string, nextCollapsed: boolean) => void;
  /// Inline-rename commit (double-click a row label to begin editing). The RAW
  /// input value is forwarded — trimming/clearing is the Rust op's job
  /// (canonical `promptRename` passes the raw NSTextField value too; a single
  /// mutation path). Rows expose the rename affordance only when provided.
  onRenameWorkspace?: (workspaceId: string, title: string) => void;
  /// Seeds the editing state so static-markup tests can render the rename
  /// input without dispatching DOM events (the established test constraint).
  defaultEditingWorkspaceId?: string;
  /// Seeds the context menu so static-markup tests can render it without a DOM
  /// event harness. Live UI opens the same menu from right-click or More.
  defaultContextMenuTarget?: WorkspaceContextMenuTargetKey;
  /// Shared context-menu action sink. Rename is handled locally by switching
  /// the row into inline-edit mode; session mutations flow through this sink.
  onContextMenuAction?: (action: WorkspaceContextMenuAction) => void;
}

/// Click-modifier extraction, kept pure so tests can assert it directly
/// (renderToStaticMarkup cannot dispatch mouse events — the renameActionForKey
/// precedent). `toggle` maps canonical Cmd to the Windows Ctrl chord; metaKey
/// is kept for parity on hosts that surface it.
export function clickModifiers(event: {
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}): WorkspaceClickModifiers {
  return { shift: event.shiftKey, toggle: event.ctrlKey || event.metaKey };
}

/// Keyboard policy for the inline-rename input, kept pure so tests can assert
/// it directly (renderToStaticMarkup cannot dispatch key events): Enter commits,
/// Escape cancels, anything else is left to the input.
export function renameActionForKey(key: string): "commit" | "cancel" | null {
  if (key === "Enter") {
    return "commit";
  }
  if (key === "Escape") {
    return "cancel";
  }
  return null;
}

function classNames(...parts: (string | false | undefined)[]): string {
  return parts.filter((p): p is string => Boolean(p)).join(" ");
}

export const SIDEBAR_DRAG_CLEAR_EVENTS = {
  windowImmediate: ["dragend", "blur"],
  windowDeferred: ["drop"],
  documentImmediate: ["visibilitychange"],
} as const;

function dropEdgeForPointer(
  currentTarget: EventTarget & HTMLLIElement,
  clientY: number,
): SidebarDropEdge {
  const bounds = currentTarget.getBoundingClientRect();
  return clientY - bounds.top < bounds.height / 2 ? "top" : "bottom";
}

function accentStyle(accentColor?: string): CSSProperties | undefined {
  return accentColor
    ? ({ "--cmux-sidebar-accent": accentColor } as CSSProperties)
    : undefined;
}

function WorkspaceRowBadges({
  badges,
  branchSummaryText,
  branchLayout = "vertical",
  makePullRequestsClickable = true,
  openPullRequestLinksInCmuxBrowser = false,
  openPortLinksInCmuxBrowser = false,
  onOpenUrlInCmuxBrowser,
}: {
  badges: readonly WorkspaceBadgeDescriptor[];
  branchSummaryText?: string | null;
  branchLayout?: SidebarBranchLayout;
  makePullRequestsClickable?: boolean;
  openPullRequestLinksInCmuxBrowser?: boolean;
  openPortLinksInCmuxBrowser?: boolean;
  onOpenUrlInCmuxBrowser?: (url: string) => void;
}) {
  if (badges.length === 0) {
    return null;
  }

  return (
    <span className="cmux-sidebar-row-badges" aria-label="Workspace badges">
      {branchLayout === "inline" && branchSummaryText ? (
        <span
          className="cmux-sidebar-row-badge cmux-sidebar-row-badge--branch"
          title={branchSummaryText}
        >
          {branchSummaryText}
        </span>
      ) : null}
      {badges.map((badge) => {
        if (badge.kind === "branch") {
          if (branchLayout === "inline") {
            return null;
          }
          return (
            <span
              key={badge.id}
              className="cmux-sidebar-row-badge cmux-sidebar-row-badge--branch"
              title={badge.name}
            >
              {badge.label}
            </span>
          );
        }
        if (badge.kind === "port") {
          return (
            <a
              key={badge.id}
              className="cmux-sidebar-row-badge cmux-sidebar-row-badge--port"
              href={badge.url}
              title={`Open localhost:${badge.port}`}
              onClick={(event) => {
                event.stopPropagation();
                if (openPortLinksInCmuxBrowser && onOpenUrlInCmuxBrowser) {
                  event.preventDefault();
                  onOpenUrlInCmuxBrowser(badge.url);
                }
              }}
            >
              {badge.label}
            </a>
          );
        }
        if (badge.kind === "remote") {
          return (
            <span
              key={badge.id}
              className={classNames(
                "cmux-sidebar-row-badge",
                "cmux-sidebar-row-badge--remote",
                `is-${badge.status}`,
              )}
              title={badge.title}
            >
              {badge.label}
              <span className="cmux-sidebar-row-badge-status">
                {badge.statusLabel}
              </span>
            </span>
          );
        }
        if (badge.kind === "shellActivity") {
          return (
            <span
              key={badge.id}
              className="cmux-sidebar-row-badge cmux-sidebar-row-badge--shell-activity"
              title={badge.title}
            >
              {badge.label}
              <span className="cmux-sidebar-row-badge-status">
                {badge.statusLabel}
              </span>
            </span>
          );
        }
        if (!makePullRequestsClickable) {
          return (
            <span
              key={badge.id}
              className={classNames(
                "cmux-sidebar-row-badge",
                "cmux-sidebar-row-badge--pull-request",
                badge.tone === "secondaryStale" && "is-stale",
              )}
              title={`${badge.label} ${badge.statusLabel}`}
            >
              {badge.label}
              <span className="cmux-sidebar-row-badge-status">
                {badge.statusLabel}
              </span>
            </span>
          );
        }
        return (
          <a
            key={badge.id}
            className={classNames(
              "cmux-sidebar-row-badge",
              "cmux-sidebar-row-badge--pull-request",
              badge.tone === "secondaryStale" && "is-stale",
            )}
            href={badge.url}
            title={`${badge.label} ${badge.statusLabel}`}
            onClick={(event) => {
              event.stopPropagation();
              if (openPullRequestLinksInCmuxBrowser && onOpenUrlInCmuxBrowser) {
                event.preventDefault();
                onOpenUrlInCmuxBrowser(badge.url);
              }
            }}
          >
            {badge.label}
            <span className="cmux-sidebar-row-badge-status">
              {badge.statusLabel}
            </span>
          </a>
        );
      })}
    </span>
  );
}

function normalizedProgressValue(progress: WorkspaceProgress): number {
  if (!Number.isFinite(progress.value)) {
    return 0;
  }
  return Math.min(1, Math.max(0, progress.value));
}

function WorkspaceRowProgress({
  progress,
}: {
  progress?: WorkspaceProgress;
}) {
  if (progress === undefined) {
    return null;
  }
  const value = normalizedProgressValue(progress);
  const percent = Math.round(value * 100);
  const label = progress.label?.trim();
  const title = label ? `${label} ${percent}%` : `${percent}%`;
  return (
    <span
      className="cmux-sidebar-row-progress"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-label={label ? `Workspace progress: ${label}` : "Workspace progress"}
      title={title}
    >
      <span className="cmux-sidebar-row-progress-track">
        <span
          className="cmux-sidebar-row-progress-fill"
          style={{ width: `${percent}%` }}
        />
      </span>
      <span className="cmux-sidebar-row-progress-label">
        {label ? `${label} ${percent}%` : `${percent}%`}
      </span>
    </span>
  );
}

function WorkspaceRowSidebarDetails({
  details,
}: {
  details?: WorkspaceSidebarDetails;
}) {
  if (
    details === undefined ||
    (details.statusEntries.length === 0 &&
      details.metadataEntries.length === 0 &&
      details.metadataBlocks.length === 0 &&
      details.logEntries.length === 0)
  ) {
    return null;
  }
  return (
    <span className="cmux-sidebar-row-details" aria-label="Workspace details">
      {details.statusEntries.length > 0 ? (
        <span className="cmux-sidebar-row-statuses">
          {details.statusEntries.map((entry) => (
            <span
              key={entry.key}
              className="cmux-sidebar-row-status"
              title={`${entry.key}: ${entry.value}`}
            >
              <span className="cmux-sidebar-row-status-key">{entry.key}</span>
              <span className="cmux-sidebar-row-status-value">{entry.value}</span>
            </span>
          ))}
        </span>
      ) : null}
      {details.metadataEntries.length > 0 ? (
        <span className="cmux-sidebar-row-metadata">
          {details.metadataEntries.map((entry) => {
            const title = [
              `${entry.key}: ${entry.value}`,
              entry.format ? `format=${entry.format}` : undefined,
              entry.url,
            ]
              .filter(Boolean)
              .join(" ");
            const content = (
              <>
                {entry.icon ? (
                  <span className="cmux-sidebar-row-meta-icon">{entry.icon}</span>
                ) : null}
                <span className="cmux-sidebar-row-meta-key">{entry.key}</span>
                <span className="cmux-sidebar-row-meta-value">
                  {entry.value}
                </span>
              </>
            );
            return entry.url ? (
              <a
                key={entry.key}
                className="cmux-sidebar-row-meta"
                href={entry.url}
                title={title}
                onClick={(event) => event.stopPropagation()}
              >
                {content}
              </a>
            ) : (
              <span
                key={entry.key}
                className="cmux-sidebar-row-meta"
                title={title}
              >
                {content}
              </span>
            );
          })}
        </span>
      ) : null}
      {details.metadataBlocks.slice(0, 2).map((entry) => (
        <span
          key={entry.key}
          className="cmux-sidebar-row-meta-block"
          title={`${entry.key}: ${entry.markdown}`}
        >
          <span className="cmux-sidebar-row-meta-key">{entry.key}</span>
          <span className="cmux-sidebar-row-meta-value">{entry.markdown}</span>
        </span>
      ))}
      {details.logEntries.slice(0, 2).map((entry, index) => (
        <span
          key={`${entry.createdAt}:${index}:${entry.message}`}
          className="cmux-sidebar-row-log"
          title={`[${entry.level}] ${entry.message}`}
        >
          <span className="cmux-sidebar-row-log-level">[{entry.level}]</span>
          <span className="cmux-sidebar-row-log-message">{entry.message}</span>
        </span>
      ))}
    </span>
  );
}

function WorkspaceContextMenu({
  target,
  isOpen,
  onOpen,
  onClose,
  onAction,
}: {
  target: WorkspaceContextMenuTarget;
  isOpen: boolean;
  onOpen?: () => void;
  onClose?: () => void;
  onAction?: (action: WorkspaceContextMenuAction) => void;
}) {
  if (onAction === undefined) {
    return null;
  }

  const title =
    target.kind === "workspace"
      ? `More actions for ${target.title}`
      : `More actions for group ${target.title}`;

  return (
    <span className="cmux-sidebar-context">
      <button
        type="button"
        className="cmux-sidebar-context-trigger"
        draggable={false}
        title={title}
        aria-label={title}
        aria-haspopup="menu"
        aria-expanded={isOpen ? "true" : "false"}
        onClick={(event) => {
          event.stopPropagation();
          onOpen?.();
        }}
      >
        ...
      </button>
      {isOpen ? (
        <span className="cmux-sidebar-context-menu" role="menu">
          {buildWorkspaceContextMenuItems(target).map((item) => (
            <button
              key={item.id}
              type="button"
              role="menuitem"
              className={classNames(
                "cmux-sidebar-context-item",
                item.destructive && "is-destructive",
                item.dividerBefore && "has-divider",
              )}
              disabled={item.disabled}
              onClick={(event) => {
                event.stopPropagation();
                if (!item.disabled) {
                  onAction(item.action);
                  onClose?.();
                }
              }}
            >
              {item.label}
            </button>
          ))}
        </span>
      ) : null}
    </span>
  );
}

function GroupHeaderRow({
  item,
  accentColor,
  isSelected,
  isMultiSelected,
  isUnread,
  isDragging,
  dropIndicatorEdge,
  contextTarget,
  isContextMenuOpen,
  onSelect,
  onOpenContextMenu,
  onContextMenuAction,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
  onToggleCollapsed,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  accentColor?: string;
  isSelected: boolean;
  isMultiSelected: boolean;
  isUnread: boolean;
  isDragging: boolean;
  dropIndicatorEdge?: SidebarDropEdge;
  contextTarget: WorkspaceContextMenuTarget;
  isContextMenuOpen: boolean;
  onSelect?: (workspaceId: string, modifiers: WorkspaceClickModifiers) => void;
  onOpenContextMenu?: (target: WorkspaceContextMenuTarget) => void;
  onContextMenuAction?: (action: WorkspaceContextMenuAction) => void;
  onDragStart?: (workspaceId: string, event: DragEvent<HTMLLIElement>) => void;
  onDragOver?: (workspaceId: string, edge: SidebarDropEdge) => void;
  onDrop?: (workspaceId: string, edge: SidebarDropEdge) => void;
  onDragEnd?: () => void;
  onToggleCollapsed?: (groupId: string, nextCollapsed: boolean) => void;
}) {
  const { group, memberWorkspaceIds } = item;
  return (
    <li
      className={classNames(
        "cmux-sidebar-group-header",
        accentColor !== undefined && "has-accent",
        group.isCollapsed && "is-collapsed",
        group.isPinned && "is-pinned",
        isSelected && "is-selected",
        isMultiSelected && "is-multi-selected",
        isUnread && "is-unread",
        isDragging && "is-dragging",
      )}
      style={accentStyle(accentColor)}
      data-group-id={group.id}
      data-collapsed={group.isCollapsed ? "true" : "false"}
      aria-expanded={group.isCollapsed ? "false" : "true"}
      aria-selected={isSelected || isMultiSelected ? "true" : "false"}
      draggable={onDragStart !== undefined}
      onClick={(event) =>
        // The header IS the anchor's row, so a modified header click
        // selects/extends via the anchor workspace.
        onSelect?.(group.anchorWorkspaceId, clickModifiers(event))
      }
      onContextMenu={(event) => {
        if (onContextMenuAction === undefined) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        onOpenContextMenu?.(contextTarget);
      }}
      onDragStart={(event) => onDragStart?.(group.anchorWorkspaceId, event)}
      onDragOver={(event) => {
        if (!onDragOver) {
          return;
        }
        event.preventDefault();
        onDragOver(group.anchorWorkspaceId, dropEdgeForPointer(event.currentTarget, event.clientY));
      }}
      onDrop={(event) => {
        if (!onDrop) {
          return;
        }
        event.preventDefault();
        onDrop(group.anchorWorkspaceId, dropEdgeForPointer(event.currentTarget, event.clientY));
      }}
      onDragEnd={() => onDragEnd?.()}
    >
      {accentColor !== undefined ? (
        <span className="cmux-sidebar-accent-pill" aria-hidden="true" />
      ) : null}
      {dropIndicatorEdge ? (
        <span
          className={classNames(
            "cmux-sidebar-drop-indicator",
            dropIndicatorEdge === "top"
              ? "cmux-sidebar-drop-indicator--top"
              : "cmux-sidebar-drop-indicator--bottom",
          )}
        />
      ) : null}
      <button
        type="button"
        className="cmux-sidebar-group-chevron"
        draggable={false}
        aria-label={group.isCollapsed ? "Expand group" : "Collapse group"}
        onClick={(event) => {
          // Chevron toggles collapse without selecting the anchor (canonical:
          // separate tap targets in SidebarWorkspaceGroupHeaderView).
          event.stopPropagation();
          onToggleCollapsed?.(group.id, !group.isCollapsed);
        }}
      >
        <Icon name={group.isCollapsed ? "arrow" : "expand"} />
      </button>
      <span className="cmux-sidebar-group-name">{group.name}</span>
      {isUnread ? (
        <span className="cmux-sidebar-unread-dot" aria-label="Unread workspace" />
      ) : null}
      <span className="cmux-sidebar-group-count">
        {memberWorkspaceIds.length}
      </span>
      <WorkspaceContextMenu
        target={contextTarget}
        isOpen={isContextMenuOpen}
        onOpen={() => onOpenContextMenu?.(contextTarget)}
        onAction={onContextMenuAction}
      />
    </li>
  );
}

function WorkspaceRowItem({
  item,
  accentColor,
  isSelected,
  isMultiSelected,
  isUnread,
  title,
  wrapWorkspaceTitles,
  description,
  badges,
  progress,
  details,
  branchLayout,
  makePullRequestsClickable,
  openPullRequestLinksInCmuxBrowser,
  openPortLinksInCmuxBrowser,
  onOpenUrlInCmuxBrowser,
  canClose,
  isEditing,
  isDragging,
  dropIndicatorEdge,
  contextTarget,
  isContextMenuOpen,
  onSelect,
  onOpenContextMenu,
  onContextMenuAction,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
  onClose,
  onSetPinned,
  onBeginRename,
  onCommitRename,
  onCancelRename,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  accentColor?: string;
  isSelected: boolean;
  isMultiSelected: boolean;
  isUnread: boolean;
  title: string;
  wrapWorkspaceTitles?: boolean;
  description?: string;
  badges?: WorkspaceBadges;
  progress?: WorkspaceProgress;
  details?: WorkspaceSidebarDetails;
  branchLayout?: SidebarBranchLayout;
  makePullRequestsClickable?: boolean;
  openPullRequestLinksInCmuxBrowser?: boolean;
  openPortLinksInCmuxBrowser?: boolean;
  onOpenUrlInCmuxBrowser?: (url: string) => void;
  canClose: boolean;
  isEditing: boolean;
  isDragging: boolean;
  dropIndicatorEdge?: SidebarDropEdge;
  contextTarget: WorkspaceContextMenuTarget;
  isContextMenuOpen: boolean;
  onSelect?: (workspaceId: string, modifiers: WorkspaceClickModifiers) => void;
  onOpenContextMenu?: (target: WorkspaceContextMenuTarget) => void;
  onContextMenuAction?: (action: WorkspaceContextMenuAction) => void;
  onDragStart?: (workspaceId: string, event: DragEvent<HTMLLIElement>) => void;
  onDragOver?: (workspaceId: string, edge: SidebarDropEdge) => void;
  onDrop?: (workspaceId: string, edge: SidebarDropEdge) => void;
  onDragEnd?: () => void;
  onClose?: (workspaceId: string) => void;
  onSetPinned?: (workspaceId: string, pinned: boolean) => void;
  onBeginRename?: (workspaceId: string) => void;
  onCommitRename?: (workspaceId: string, title: string) => void;
  onCancelRename?: () => void;
}) {
  const { workspace } = item;
  return (
    <li
      className={classNames(
        "cmux-sidebar-row",
        accentColor !== undefined && "has-accent",
        workspace.isPinned && "is-pinned",
        isSelected && "is-selected",
        isMultiSelected && "is-multi-selected",
        isUnread && "is-unread",
        isDragging && "is-dragging",
      )}
      style={accentStyle(accentColor)}
      data-workspace-id={workspace.id}
      aria-selected={isSelected || isMultiSelected ? "true" : "false"}
      draggable={!isEditing && onDragStart !== undefined}
      onClick={(event) => onSelect?.(workspace.id, clickModifiers(event))}
      onContextMenu={(event) => {
        if (onContextMenuAction === undefined) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        onOpenContextMenu?.(contextTarget);
      }}
      onDragStart={(event) => onDragStart?.(workspace.id, event)}
      onDragOver={(event) => {
        if (!onDragOver) {
          return;
        }
        event.preventDefault();
        onDragOver(workspace.id, dropEdgeForPointer(event.currentTarget, event.clientY));
      }}
      onDrop={(event) => {
        if (!onDrop) {
          return;
        }
        event.preventDefault();
        onDrop(workspace.id, dropEdgeForPointer(event.currentTarget, event.clientY));
      }}
      onDragEnd={() => onDragEnd?.()}
    >
      {accentColor !== undefined ? (
        <span className="cmux-sidebar-accent-pill" aria-hidden="true" />
      ) : null}
      {dropIndicatorEdge ? (
        <span
          className={classNames(
            "cmux-sidebar-drop-indicator",
            dropIndicatorEdge === "top"
              ? "cmux-sidebar-drop-indicator--top"
              : "cmux-sidebar-drop-indicator--bottom",
          )}
        />
      ) : null}
      <span
        className={classNames(
          "cmux-icon",
          "cmux-sidebar-row-icon",
          workspace.isPinned && "cmux-sidebar-workspace-pin",
        )}
        aria-hidden="true"
      >
        <Icon name={workspace.isPinned ? "files" : "classic"} />
      </span>
      {isUnread ? (
        <span className="cmux-sidebar-unread-dot" aria-label="Unread workspace" />
      ) : null}
      {isEditing ? (
        <input
          type="text"
          className="cmux-sidebar-row-rename"
          // Prefill = the resolved row title — parity with canonical
          // `promptRename`'s `tab.customTitle ?? tab.title`
          // (ContentView.swift:14944): titleForWorkspace already resolves
          // custom_title || process_title || "Terminal".
          defaultValue={title}
          autoFocus
          // Canonical promptRename select-alls the prefill (selectText,
          // ContentView.swift:14954) so typing replaces the old title.
          onFocus={(event) => event.currentTarget.select()}
          aria-label={`Rename ${title}`}
          draggable={false}
          // Never let a click inside the editor re-fire the row's select.
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            const action = renameActionForKey(event.key);
            if (action === "commit") {
              onCommitRename?.(workspace.id, event.currentTarget.value);
            } else if (action === "cancel") {
              onCancelRename?.();
            }
          }}
          // Blur CANCELS: the canonical rename is a modal that commits only on
          // the explicit Rename button (`guard response ==
          // .alertFirstButtonReturn`, ContentView.swift:14957) — dismissal
          // without affirmation discards, so losing focus maps to cancel.
          onBlur={() => onCancelRename?.()}
        />
      ) : (
        <div
          className={classNames(
            "cmux-sidebar-row-text",
            wrapWorkspaceTitles && "can-wrap-title",
            description !== undefined && "has-description",
          )}
          onDoubleClick={
            onBeginRename ? () => onBeginRename(workspace.id) : undefined
          }
        >
          <span className="cmux-sidebar-row-label">{title}</span>
          {description !== undefined ? (
            <span className="cmux-sidebar-row-description">{description}</span>
          ) : null}
          <WorkspaceRowBadges
            badges={badges?.badges ?? []}
            branchSummaryText={badges?.branchSummaryText}
            branchLayout={branchLayout}
            makePullRequestsClickable={makePullRequestsClickable}
            openPullRequestLinksInCmuxBrowser={openPullRequestLinksInCmuxBrowser}
            openPortLinksInCmuxBrowser={openPortLinksInCmuxBrowser}
            onOpenUrlInCmuxBrowser={onOpenUrlInCmuxBrowser}
          />
          <WorkspaceRowProgress progress={progress} />
          <WorkspaceRowSidebarDetails details={details} />
        </div>
      )}
      {onSetPinned ? (
        <button
          type="button"
          className="cmux-sidebar-row-pin"
          draggable={false}
          // Canonical action labels: "Pin Workspace"/"Unpin Workspace"
          // (WorkspaceActionDispatcher.swift:166-170).
          title={workspace.isPinned ? "Unpin Workspace" : "Pin Workspace"}
          aria-label={workspace.isPinned ? `Unpin ${title}` : `Pin ${title}`}
          onClick={(event) => {
            // Don't let the row's select handler fire on the pin toggle.
            event.stopPropagation();
            onSetPinned(workspace.id, !workspace.isPinned);
          }}
        >
          <Icon name="files" />
        </button>
      ) : null}
      <WorkspaceContextMenu
        target={contextTarget}
        isOpen={isContextMenuOpen}
        onOpen={() => onOpenContextMenu?.(contextTarget)}
        onAction={onContextMenuAction}
      />
      {canClose ? (
        <button
          type="button"
          className={classNames(
            "cmux-sidebar-row-close",
            isSelected && "is-visible",
          )}
          draggable={false}
          title="Close workspace"
          aria-label={`Close ${title}`}
          onClick={(event) => {
            // Don't let the row's select handler fire on close.
            event.stopPropagation();
            onClose?.(workspace.id);
          }}
        >
          ✕
        </button>
      ) : null}
    </li>
  );
}

export function WorkspaceList({
  items,
  selectedWorkspaceIds,
  multiSelectedWorkspaceIds,
  unreadWorkspaceIds,
  titleForWorkspace,
  wrapWorkspaceTitles = false,
  descriptionForWorkspace,
  badgesForWorkspace,
  progressForWorkspace,
  detailsForWorkspace,
  branchLayout = "vertical",
  makePullRequestsClickable = true,
  openPullRequestLinksInCmuxBrowser = false,
  openPortLinksInCmuxBrowser = false,
  onOpenUrlInCmuxBrowser,
  canCloseWorkspaces,
  onSelectWorkspace,
  onReorderWorkspace,
  onCloseWorkspace,
  onSetWorkspacePinned,
  onToggleGroupCollapsed,
  onRenameWorkspace,
  defaultEditingWorkspaceId,
  defaultContextMenuTarget,
  onContextMenuAction,
}: WorkspaceListProps) {
  const selected = selectedWorkspaceIds ?? new Set<string>();
  const multiSelected = multiSelectedWorkspaceIds ?? new Set<string>();
  const unread = unreadWorkspaceIds ?? new Set<string>();
  // Active precedence: an active row never doubles as multi-selected
  // (SidebarAppearanceSupport.swift:312-336 checks isActive first). Hidden
  // collapsed members that are multi-selected simply don't render; a header
  // reflects only its anchor.
  const isMultiSelectedRow = (workspaceId: string) =>
    multiSelected.has(workspaceId) && !selected.has(workspaceId);
  // At most one row edits at a time. Group ANCHOR rows are not renamable here:
  // the header suppresses the anchor's plain row, and canonical group headers
  // rename the GROUP (`renameWorkspaceGroup`) — a different op, out of scope.
  const [editingWorkspaceId, setEditingWorkspaceId] = useState<string | null>(
    defaultEditingWorkspaceId ?? null,
  );
  const [contextMenuTarget, setContextMenuTarget] =
    useState<WorkspaceContextMenuTargetKey | null>(
      defaultContextMenuTarget ?? null,
    );
  const [draggedWorkspaceId, setDraggedWorkspaceId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<{
    workspaceId: string;
    edge: SidebarDropEdge;
  } | null>(null);

  const clearDragState = useCallback(() => {
    setDraggedWorkspaceId(null);
    setDropIndicator(null);
  }, []);

  const hasActiveDrag = draggedWorkspaceId !== null || dropIndicator !== null;
  useEffect(() => {
    if (!hasActiveDrag || typeof window === "undefined") {
      return;
    }

    let clearTimer: number | null = null;
    const clearSoon = (): void => {
      if (clearTimer !== null) {
        window.clearTimeout(clearTimer);
      }
      clearTimer = window.setTimeout(() => {
        clearTimer = null;
        clearDragState();
      }, 0);
    };
    const clearIfHidden = (): void => {
      if (document.visibilityState === "hidden") {
        clearDragState();
      }
    };

    window.addEventListener("dragend", clearDragState, true);
    window.addEventListener("drop", clearSoon, true);
    window.addEventListener("blur", clearDragState);
    document.addEventListener("visibilitychange", clearIfHidden);
    return () => {
      window.removeEventListener("dragend", clearDragState, true);
      window.removeEventListener("drop", clearSoon, true);
      window.removeEventListener("blur", clearDragState);
      document.removeEventListener("visibilitychange", clearIfHidden);
      if (clearTimer !== null) {
        window.clearTimeout(clearTimer);
      }
    };
  }, [clearDragState, hasActiveDrag]);

  const canDrag = onReorderWorkspace !== undefined;
  const dispatchContextMenuAction = (action: WorkspaceContextMenuAction) => {
    if (action.kind === "beginRenameWorkspace") {
      setEditingWorkspaceId(action.workspaceId);
      setContextMenuTarget(null);
      return;
    }
    onContextMenuAction?.(action);
    setContextMenuTarget(null);
  };
  const openContextMenu = (target: WorkspaceContextMenuTarget) => {
    const key = contextMenuKeyForTarget(target);
    setContextMenuTarget((current) =>
      contextMenuTargetMatches(current, target) ? null : key,
    );
  };
  const groupColorById = new Map<string, string>();
  for (const item of items) {
    if (item.kind === "groupHeader" && item.group.customColor !== undefined) {
      groupColorById.set(item.group.id, item.group.customColor);
    }
  }
  return (
    <ul className="cmux-sidebar-list cmux-sidebar-workspace-list">
      {items.map((item) => {
        if (item.kind === "groupHeader") {
          const workspaceId = item.group.anchorWorkspaceId;
          const contextTarget: WorkspaceContextMenuTarget = {
            kind: "group",
            groupId: item.group.id,
            title: item.group.name,
            isCollapsed: item.group.isCollapsed,
            memberWorkspaceIds: item.memberWorkspaceIds,
            canCreateWorkspace: onContextMenuAction !== undefined,
            canCollapse: onToggleGroupCollapsed !== undefined,
            canClose:
              (canCloseWorkspaces ?? false) &&
              onContextMenuAction !== undefined,
          };
          return (
            <GroupHeaderRow
              key={`group:${item.group.id}`}
              item={item}
              accentColor={item.group.customColor}
              isSelected={selected.has(workspaceId)}
              isMultiSelected={isMultiSelectedRow(workspaceId)}
              isUnread={item.memberWorkspaceIds.some((id) => unread.has(id))}
              isDragging={draggedWorkspaceId === workspaceId}
              dropIndicatorEdge={
                dropIndicator?.workspaceId === workspaceId
                  ? dropIndicator.edge
                  : undefined
              }
              contextTarget={contextTarget}
              isContextMenuOpen={contextMenuTargetMatches(
                contextMenuTarget,
                contextTarget,
              )}
              onSelect={onSelectWorkspace}
              onOpenContextMenu={openContextMenu}
              onContextMenuAction={dispatchContextMenuAction}
              onDragStart={
                canDrag
                  ? (draggedWorkspaceId, event) => {
                      event.dataTransfer.effectAllowed = "move";
                      event.dataTransfer.setData("text/plain", draggedWorkspaceId);
                      setDraggedWorkspaceId(draggedWorkspaceId);
                      setDropIndicator(null);
                    }
                  : undefined
              }
              onDragOver={
                canDrag
                  ? (workspaceId, edge) => {
                      if (draggedWorkspaceId === null) {
                        return;
                      }
                      setDropIndicator({ workspaceId, edge });
                    }
                  : undefined
              }
              onDrop={
                canDrag
                  ? (workspaceId, edge) => {
                      if (draggedWorkspaceId === null) {
                        return;
                      }
                      onReorderWorkspace?.(draggedWorkspaceId, workspaceId, edge);
                      clearDragState();
                    }
                  : undefined
              }
              onDragEnd={canDrag ? clearDragState : undefined}
              onToggleCollapsed={onToggleGroupCollapsed}
            />
          );
        }
        const workspaceId = item.workspace.id;
        const title = titleForWorkspace?.(workspaceId) ?? workspaceId;
        const contextTarget: WorkspaceContextMenuTarget = {
          kind: "workspace",
          workspaceId,
          title,
          isPinned: item.workspace.isPinned,
          canCreateWorkspace: onContextMenuAction !== undefined,
          canRename: onRenameWorkspace !== undefined,
          canPin: onSetWorkspacePinned !== undefined,
          canClose:
            (canCloseWorkspaces ?? false) && onContextMenuAction !== undefined,
          canCloseOthers:
            (canCloseWorkspaces ?? false) && onContextMenuAction !== undefined,
        };
        return (
          <WorkspaceRowItem
            key={`workspace:${workspaceId}`}
            item={item}
            accentColor={
              item.workspace.customColor ??
              (item.workspace.groupId !== undefined
                ? groupColorById.get(item.workspace.groupId)
                : undefined)
            }
            isSelected={selected.has(workspaceId)}
            isMultiSelected={isMultiSelectedRow(workspaceId)}
            isUnread={unread.has(workspaceId)}
            title={title}
            wrapWorkspaceTitles={wrapWorkspaceTitles}
            description={descriptionForWorkspace?.(workspaceId)}
            badges={badgesForWorkspace?.(workspaceId)}
            progress={progressForWorkspace?.(workspaceId)}
            details={detailsForWorkspace?.(workspaceId)}
            branchLayout={branchLayout}
            makePullRequestsClickable={makePullRequestsClickable}
            openPullRequestLinksInCmuxBrowser={openPullRequestLinksInCmuxBrowser}
            openPortLinksInCmuxBrowser={openPortLinksInCmuxBrowser}
            onOpenUrlInCmuxBrowser={onOpenUrlInCmuxBrowser}
            canClose={canCloseWorkspaces ?? false}
            isEditing={editingWorkspaceId === workspaceId}
            isDragging={draggedWorkspaceId === workspaceId}
            dropIndicatorEdge={
              dropIndicator?.workspaceId === workspaceId
                ? dropIndicator.edge
                : undefined
            }
            contextTarget={contextTarget}
            isContextMenuOpen={contextMenuTargetMatches(
              contextMenuTarget,
              contextTarget,
            )}
            onSelect={onSelectWorkspace}
            onOpenContextMenu={openContextMenu}
            onContextMenuAction={dispatchContextMenuAction}
            onDragStart={
              canDrag
                ? (draggedWorkspaceId, event) => {
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData("text/plain", draggedWorkspaceId);
                    setDraggedWorkspaceId(draggedWorkspaceId);
                    setDropIndicator(null);
                  }
                : undefined
            }
            onDragOver={
              canDrag
                ? (workspaceId, edge) => {
                    if (draggedWorkspaceId === null) {
                      return;
                    }
                    setDropIndicator({ workspaceId, edge });
                  }
                : undefined
            }
            onDrop={
              canDrag
                ? (workspaceId, edge) => {
                    if (draggedWorkspaceId === null) {
                      return;
                    }
                    onReorderWorkspace?.(draggedWorkspaceId, workspaceId, edge);
                    clearDragState();
                  }
                : undefined
            }
            onDragEnd={canDrag ? clearDragState : undefined}
            onClose={onCloseWorkspace}
            onSetPinned={onSetWorkspacePinned}
            onBeginRename={onRenameWorkspace ? setEditingWorkspaceId : undefined}
            onCommitRename={(workspaceId, title) => {
              onRenameWorkspace?.(workspaceId, title);
              setEditingWorkspaceId(null);
            }}
            onCancelRename={() => setEditingWorkspaceId(null)}
          />
        );
      })}
    </ul>
  );
}
