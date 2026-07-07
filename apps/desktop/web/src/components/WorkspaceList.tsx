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
//   headers in this first cut. Collapse-toggle wiring is A5's slice.
//
// Reuses the shared `@cmux/webviews` Icon for glyphs. The icon set has no
// dedicated folder/right-chevron, so a collapsed group shows the right-pointing
// `arrow` glyph and an expanded group the down-pointing `expand` glyph.

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
}

function classNames(...parts: (string | false | undefined)[]): string {
  return parts.filter((p): p is string => Boolean(p)).join(" ");
}

function GroupHeaderRow({
  item,
  isSelected,
  onSelect,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  isSelected: boolean;
  onSelect?: (workspaceId: string) => void;
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
      <span className="cmux-icon cmux-sidebar-group-chevron" aria-hidden="true">
        <Icon name={group.isCollapsed ? "arrow" : "expand"} />
      </span>
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
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  title: string;
  isSelected: boolean;
  canClose: boolean;
  onSelect?: (workspaceId: string) => void;
  onClose?: (workspaceId: string) => void;
}) {
  const { workspace } = item;
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
    >
      <span className="cmux-icon cmux-sidebar-row-icon" aria-hidden="true">
        <Icon name={workspace.isPinned ? "files" : "classic"} />
      </span>
      <span className="cmux-sidebar-row-label">{title}</span>
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
          />
        );
      })}
    </ul>
  );
}
