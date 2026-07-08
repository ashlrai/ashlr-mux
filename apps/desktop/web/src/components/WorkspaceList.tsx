// Renders the projected `SidebarWorkspaceRenderItem[]` (see
// `../sidebar/renderItems`) into the sidebar: group headers (member count +
// collapse chevron) and workspace rows (pin state + selection + close). Pure /
// prop-driven — collapse suppression already happened in `renderItems`, so a
// collapsed group simply arrives here without its member rows.
//
// Interaction contract (canonical `CmuxSidebar` row behavior):
// - Clicking a workspace row selects it; clicking a group header selects the
//   group's anchor workspace (the header IS the anchor's row — the anchor is
//   suppressed as a plain row by the projection).
// - The ✕ appears only when `canCloseWorkspaces` (canonical
//   `TabManager.closeWorkspace` is a no-op on the last tab) and never on group
//   headers in this first cut.
// - The header chevron toggles the group's collapse state (A5) without firing
//   the header's select.
// - Double-clicking a row label opens an inline rename editor (A6): Enter or
//   blur commits (an empty commit clears the custom title — canonical
//   setCustomTitle), Escape cancels.
//
// Reuses the shared `@cmux/webviews` Icon for glyphs. The icon set has no
// dedicated folder/right-chevron, so a collapsed group shows the right-pointing
// `arrow` glyph and an expanded group the down-pointing `expand` glyph.

import { useState } from "react";

import { Icon } from "@cmux/webviews/src/icons";

import type { SidebarWorkspaceRenderItem } from "../sidebar/renderItems";

export interface WorkspaceListProps {
  /// The drawable items, already projected by `renderItems`.
  items: readonly SidebarWorkspaceRenderItem[];
  /// Ids of currently-selected workspaces. A group header counts as selected
  /// when its anchor workspace is selected.
  selectedWorkspaceIds?: ReadonlySet<string>;
  /// Display titles keyed by (normalized) workspace id; rows fall back to
  /// "Terminal" when absent.
  titlesById?: ReadonlyMap<string, string>;
  /// Whether workspace rows show a close affordance (false on the sole tab).
  canCloseWorkspaces?: boolean;
  /// Select the workspace with this id (a header passes its anchor's id).
  onSelectWorkspace?: (workspaceId: string) => void;
  /// Close the workspace with this id.
  onCloseWorkspace?: (workspaceId: string) => void;
  /// Commit an inline rename (empty clears the custom title).
  onRenameWorkspace?: (workspaceId: string, title: string) => void;
  /// Pin/unpin the workspace (canonical pinned-ahead reorder).
  onSetWorkspacePinned?: (workspaceId: string, pinned: boolean) => void;
  /// Toggle a group's collapse state to `collapsed`.
  onSetGroupCollapsed?: (groupId: string, collapsed: boolean) => void;
}

function classNames(...parts: (string | false | undefined)[]): string {
  return parts.filter((p): p is string => Boolean(p)).join(" ");
}

function GroupHeaderRow({
  item,
  isSelected,
  onSelect,
  onSetCollapsed,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  isSelected: boolean;
  onSelect?: (workspaceId: string) => void;
  onSetCollapsed?: (groupId: string, collapsed: boolean) => void;
}) {
  const { group, memberWorkspaceIds } = item;
  return (
    <li
      className={classNames(
        "cmux-sidebar-group-header",
        group.isCollapsed && "is-collapsed",
        group.isPinned && "is-pinned",
        isSelected && "is-selected",
      )}
      data-group-id={group.id}
      data-collapsed={group.isCollapsed ? "true" : "false"}
      aria-expanded={group.isCollapsed ? "false" : "true"}
      onClick={() => onSelect?.(group.anchorWorkspaceId)}
    >
      <button
        type="button"
        className="cmux-icon cmux-sidebar-group-chevron"
        title={group.isCollapsed ? "Expand group" : "Collapse group"}
        aria-label={`${group.isCollapsed ? "Expand" : "Collapse"} ${group.name}`}
        onClick={(event) => {
          // Toggling collapse must not also select the anchor.
          event.stopPropagation();
          onSetCollapsed?.(group.id, !group.isCollapsed);
        }}
      >
        <Icon name={group.isCollapsed ? "arrow" : "expand"} />
      </button>
      <span className="cmux-sidebar-group-name">{group.name}</span>
      <span className="cmux-sidebar-group-count">
        {memberWorkspaceIds.length}
      </span>
    </li>
  );
}

function WorkspaceRowItem({
  item,
  title,
  isSelected,
  canClose,
  onSelect,
  onClose,
  onRename,
  onSetPinned,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  title: string;
  isSelected: boolean;
  canClose: boolean;
  onSelect?: (workspaceId: string) => void;
  onClose?: (workspaceId: string) => void;
  onRename?: (workspaceId: string, title: string) => void;
  onSetPinned?: (workspaceId: string, pinned: boolean) => void;
}) {
  const { workspace } = item;
  // Inline rename editor state: `null` when idle, else the draft text.
  const [draft, setDraft] = useState<string | null>(null);
  const commit = (): void => {
    if (draft !== null) {
      onRename?.(workspace.id, draft);
      setDraft(null);
    }
  };
  return (
    <li
      className={classNames(
        "cmux-sidebar-row",
        "cmux-sidebar-workspace-row",
        workspace.isPinned && "is-pinned",
        isSelected && "is-selected",
      )}
      data-workspace-id={workspace.id}
      aria-selected={isSelected ? "true" : "false"}
      onClick={() => onSelect?.(workspace.id)}
      onDoubleClick={() => {
        if (onRename && draft === null) {
          setDraft(title);
        }
      }}
    >
      <span className="cmux-icon cmux-sidebar-row-icon" aria-hidden="true">
        <Icon name={workspace.isPinned ? "files" : "classic"} />
      </span>
      {draft !== null ? (
        <input
          className="cmux-sidebar-row-rename"
          value={draft}
          autoFocus
          aria-label={`Rename ${title}`}
          onChange={(event) => setDraft(event.target.value)}
          onClick={(event) => event.stopPropagation()}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              commit();
            } else if (event.key === "Escape") {
              setDraft(null);
            }
          }}
        />
      ) : (
        <span className="cmux-sidebar-row-label">{title}</span>
      )}
      {onSetPinned ? (
        <button
          type="button"
          className={classNames(
            "cmux-sidebar-row-pin",
            workspace.isPinned && "is-pinned",
          )}
          title={workspace.isPinned ? "Unpin workspace" : "Pin workspace"}
          aria-label={`${workspace.isPinned ? "Unpin" : "Pin"} ${title}`}
          onClick={(event) => {
            // Don't let the row's select handler fire on pin-toggle.
            event.stopPropagation();
            onSetPinned(workspace.id, !workspace.isPinned);
          }}
        >
          ⌖
        </button>
      ) : null}
      {canClose && onClose ? (
        <button
          type="button"
          className="cmux-sidebar-row-close"
          title="Close workspace"
          aria-label={`Close ${title}`}
          onClick={(event) => {
            // Don't let the row's select handler fire on close.
            event.stopPropagation();
            onClose(workspace.id);
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
  titlesById,
  canCloseWorkspaces = false,
  onSelectWorkspace,
  onCloseWorkspace,
  onRenameWorkspace,
  onSetWorkspacePinned,
  onSetGroupCollapsed,
}: WorkspaceListProps) {
  const selected = selectedWorkspaceIds ?? new Set<string>();
  return (
    <ul className="cmux-sidebar-list cmux-sidebar-workspace-list">
      {items.map((item) => {
        if (item.kind === "groupHeader") {
          return (
            <GroupHeaderRow
              key={`group:${item.group.id}`}
              item={item}
              isSelected={selected.has(item.group.anchorWorkspaceId)}
              onSelect={onSelectWorkspace}
              onSetCollapsed={onSetGroupCollapsed}
            />
          );
        }
        return (
          <WorkspaceRowItem
            key={`workspace:${item.workspace.id}`}
            item={item}
            title={titlesById?.get(item.workspace.id) ?? "Terminal"}
            isSelected={selected.has(item.workspace.id)}
            canClose={canCloseWorkspaces}
            onSelect={onSelectWorkspace}
            onClose={onCloseWorkspace}
            onRename={onRenameWorkspace}
            onSetPinned={onSetWorkspacePinned}
          />
        );
      })}
    </ul>
  );
}
