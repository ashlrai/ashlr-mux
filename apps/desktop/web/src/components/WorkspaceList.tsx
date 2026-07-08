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

import { useState } from "react";

import { Icon } from "@cmux/webviews/src/icons";

import type { SidebarWorkspaceRenderItem } from "../sidebar/renderItems";
import type { WorkspaceClickModifiers } from "../sidebar/selection";

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
  /// Resolves a workspace id to its row label; defaults to the id itself.
  titleForWorkspace?: (workspaceId: string) => string;
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

function GroupHeaderRow({
  item,
  isSelected,
  isMultiSelected,
  onSelect,
  onToggleCollapsed,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "groupHeader" }>;
  isSelected: boolean;
  isMultiSelected: boolean;
  onSelect?: (workspaceId: string, modifiers: WorkspaceClickModifiers) => void;
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
        isMultiSelected && "is-multi-selected",
      )}
      data-group-id={group.id}
      data-collapsed={group.isCollapsed ? "true" : "false"}
      aria-expanded={group.isCollapsed ? "false" : "true"}
      aria-selected={isSelected || isMultiSelected ? "true" : "false"}
      onClick={(event) =>
        // The header IS the anchor's row, so a modified header click
        // selects/extends via the anchor workspace.
        onSelect?.(group.anchorWorkspaceId, clickModifiers(event))
      }
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
  isMultiSelected,
  title,
  canClose,
  isEditing,
  onSelect,
  onClose,
  onSetPinned,
  onBeginRename,
  onCommitRename,
  onCancelRename,
}: {
  item: Extract<SidebarWorkspaceRenderItem, { kind: "workspace" }>;
  isSelected: boolean;
  isMultiSelected: boolean;
  title: string;
  canClose: boolean;
  isEditing: boolean;
  onSelect?: (workspaceId: string, modifiers: WorkspaceClickModifiers) => void;
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
        workspace.isPinned && "is-pinned",
        isSelected && "is-selected",
        isMultiSelected && "is-multi-selected",
      )}
      data-workspace-id={workspace.id}
      aria-selected={isSelected || isMultiSelected ? "true" : "false"}
      onClick={(event) => onSelect?.(workspace.id, clickModifiers(event))}
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
        <span
          className="cmux-sidebar-row-label"
          onDoubleClick={
            onBeginRename ? () => onBeginRename(workspace.id) : undefined
          }
        >
          {title}
        </span>
      )}
      {onSetPinned ? (
        <button
          type="button"
          className="cmux-sidebar-row-pin"
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
  multiSelectedWorkspaceIds,
  titleForWorkspace,
  canCloseWorkspaces,
  onSelectWorkspace,
  onCloseWorkspace,
  onSetWorkspacePinned,
  onToggleGroupCollapsed,
  onRenameWorkspace,
  defaultEditingWorkspaceId,
}: WorkspaceListProps) {
  const selected = selectedWorkspaceIds ?? new Set<string>();
  const multiSelected = multiSelectedWorkspaceIds ?? new Set<string>();
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
  return (
    <ul className="cmux-sidebar-list cmux-sidebar-workspace-list">
      {items.map((item) => {
        if (item.kind === "groupHeader") {
          return (
            <GroupHeaderRow
              key={`group:${item.group.id}`}
              item={item}
              isSelected={selected.has(item.group.anchorWorkspaceId)}
              isMultiSelected={isMultiSelectedRow(item.group.anchorWorkspaceId)}
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
            isMultiSelected={isMultiSelectedRow(item.workspace.id)}
            title={titleForWorkspace?.(item.workspace.id) ?? item.workspace.id}
            canClose={canCloseWorkspaces ?? false}
            isEditing={editingWorkspaceId === item.workspace.id}
            onSelect={onSelectWorkspace}
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
