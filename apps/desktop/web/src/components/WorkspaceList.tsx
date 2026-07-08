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

import { Icon } from "@cmux/webviews/src/icons";

import type { SidebarWorkspaceRenderItem } from "../sidebar/renderItems";

export interface WorkspaceListProps {
  /// The drawable items, already projected by `renderItems`.
  items: readonly SidebarWorkspaceRenderItem[];
  /// Ids of currently-selected workspaces. A group header counts as selected
  /// when its anchor workspace is selected.
  selectedWorkspaceIds?: ReadonlySet<string>;
  /// Resolves a workspace id to its row label; defaults to the id itself.
  titleForWorkspace?: (workspaceId: string) => string;
  /// Whether workspace rows expose a close (✕) affordance. Canonical
  /// `TabManager.closeWorkspace` is a no-op with one workspace left, so the
  /// sole survivor hides it.
  canCloseWorkspaces?: boolean;
  /// Row activation — a group header activates its anchor workspace.
  onSelectWorkspace?: (workspaceId: string) => void;
  onCloseWorkspace?: (workspaceId: string) => void;
  /// Chevron activation — toggles the group's collapsed state.
  onToggleGroupCollapsed?: (groupId: string, nextCollapsed: boolean) => void;
}

function classNames(...parts: (string | false | undefined)[]): string {
  return parts.filter((p): p is string => Boolean(p)).join(" ");
}

function GroupHeaderRow({
  item,
  isSelected,
  onSelect,
  onToggleCollapsed,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  isSelected: boolean;
  onSelect?: (workspaceId: string) => void;
  onToggleCollapsed?: (groupId: string, nextCollapsed: boolean) => void;
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
      aria-selected={isSelected ? "true" : "false"}
      onClick={() => onSelect?.(group.anchorWorkspaceId)}
    >
      <button
        type="button"
        className="cmux-sidebar-group-chevron"
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
      <span className="cmux-sidebar-group-count">
        {memberWorkspaceIds.length}
      </span>
    </li>
  );
}

function WorkspaceRowItem({
  item,
  isSelected,
  title,
  canClose,
  onSelect,
  onClose,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  isSelected: boolean;
  title: string;
  canClose: boolean;
  onSelect?: (workspaceId: string) => void;
  onClose?: (workspaceId: string) => void;
}) {
  const { workspace } = item;
  return (
    <li
      className={classNames(
        "cmux-sidebar-row",
        workspace.isPinned && "is-pinned",
        isSelected && "is-selected",
      )}
      data-workspace-id={workspace.id}
      aria-selected={isSelected ? "true" : "false"}
      onClick={() => onSelect?.(workspace.id)}
    >
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
  titleForWorkspace,
  canCloseWorkspaces,
  onSelectWorkspace,
  onCloseWorkspace,
  onToggleGroupCollapsed,
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
              onToggleCollapsed={onToggleGroupCollapsed}
            />
          );
        }
        return (
          <WorkspaceRowItem
            key={`workspace:${item.workspace.id}`}
            item={item}
            isSelected={selected.has(item.workspace.id)}
            title={titleForWorkspace?.(item.workspace.id) ?? item.workspace.id}
            canClose={canCloseWorkspaces ?? false}
            onSelect={onSelectWorkspace}
            onClose={onCloseWorkspace}
          />
        );
      })}
    </ul>
  );
}
