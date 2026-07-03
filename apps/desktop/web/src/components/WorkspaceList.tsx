// Renders the projected `SidebarWorkspaceRenderItem[]` (see
// `../sidebar/renderItems`) into the sidebar: group headers (member count +
// collapse chevron) and workspace rows (pin state + selection). Pure/structural
// — collapse suppression already happened in `renderItems`, so a collapsed
// group simply arrives here without its member rows.
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
}

function classNames(...parts: (string | false | undefined)[]): string {
  return parts.filter((p): p is string => Boolean(p)).join(" ");
}

function GroupHeaderRow({
  item,
  isSelected,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  isSelected: boolean;
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
    >
      <span className="cmux-sidebar-group-chevron" aria-hidden="true">
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
  isSelected,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  isSelected: boolean;
}) {
  const { workspace } = item;
  return (
    <li
      className={classNames(
        "cmux-sidebar-workspace-row",
        workspace.isPinned && "is-pinned",
        isSelected && "is-selected",
      )}
      data-workspace-id={workspace.id}
      aria-selected={isSelected ? "true" : "false"}
    >
      {workspace.isPinned ? (
        <span className="cmux-sidebar-workspace-pin" aria-hidden="true">
          <Icon name="files" />
        </span>
      ) : null}
      <span className="cmux-sidebar-workspace-id">{workspace.id}</span>
    </li>
  );
}

export function WorkspaceList({
  items,
  selectedWorkspaceIds,
}: WorkspaceListProps) {
  const selected = selectedWorkspaceIds ?? new Set<string>();
  return (
    <ul className="cmux-sidebar-workspace-list">
      {items.map((item) => {
        if (item.kind === "groupHeader") {
          return (
            <GroupHeaderRow
              key={`group:${item.group.id}`}
              item={item}
              isSelected={selected.has(item.group.anchorWorkspaceId)}
            />
          );
        }
        return (
          <WorkspaceRowItem
            key={`workspace:${item.workspace.id}`}
            item={item}
            isSelected={selected.has(item.workspace.id)}
          />
        );
      })}
    </ul>
  );
}
