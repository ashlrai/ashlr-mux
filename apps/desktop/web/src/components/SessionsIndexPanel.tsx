import type {
  SessionWorkspaceLayoutSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

export interface SessionIndexEntry {
  id: string;
  index: number;
  title: string;
  description: string | null;
  directory: string | null;
  panelCount: number;
  isSelected: boolean;
  isPinned: boolean;
  hasUnread: boolean;
  layoutMode: string;
}

export interface SessionsIndexPanelProps {
  workspaces: readonly SessionWorkspaceSnapshot[];
  selectedWorkspaceIndex: number;
  onSelectWorkspace: (index: number) => void;
}

export function panelIdsInLayout(
  layout: SessionWorkspaceLayoutSnapshot | null,
): string[] {
  if (layout === null) {
    return [];
  }
  if (layout.type === "pane") {
    return layout.pane.panel_ids.length > 0
      ? layout.pane.panel_ids
      : layout.pane.selected_panel_id != null
        ? [layout.pane.selected_panel_id]
        : [];
  }
  return [
    ...panelIdsInLayout(layout.split.first),
    ...panelIdsInLayout(layout.split.second),
  ];
}

export function sessionIndexEntries(
  workspaces: readonly SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex: number,
): SessionIndexEntry[] {
  return workspaces.map((workspace, index) => {
    const panelIds = panelIdsInLayout(workspace.layout);
    return {
      id: workspace.workspace_id ?? `workspace-${index}`,
      index,
      title:
        nonBlank(workspace.custom_title) ??
        nonBlank(workspace.process_title) ??
        `Workspace ${index + 1}`,
      description: nonBlank(workspace.custom_description),
      directory: nonBlank(workspace.current_directory),
      panelCount: panelIds.length,
      isSelected: index === selectedWorkspaceIndex,
      isPinned: workspace.is_pinned === true,
      hasUnread: (workspace.panel_unreads ?? []).some((row) => row.is_unread),
      layoutMode: workspace.layout_mode === "canvas" ? "Canvas" : "Splits",
    };
  });
}

export function SessionsIndexPanel({
  workspaces,
  selectedWorkspaceIndex,
  onSelectWorkspace,
}: SessionsIndexPanelProps): React.JSX.Element {
  const entries = sessionIndexEntries(workspaces, selectedWorkspaceIndex);

  if (entries.length === 0) {
    return (
      <div className="cmux-session-index-empty">
        <strong>Vault is empty</strong>
        <span>Workspaces and resumable sessions will appear here.</span>
      </div>
    );
  }

  return (
    <section className="cmux-session-index" aria-label="Vault sessions">
      <div className="cmux-session-index-summary">
        <span>{entries.length} workspace{entries.length === 1 ? "" : "s"}</span>
        <span>{entries.reduce((sum, entry) => sum + entry.panelCount, 0)} surfaces</span>
      </div>
      <ul className="cmux-session-index-list">
        {entries.map((entry) => (
          <li key={entry.id}>
            <button
              type="button"
              className={
                entry.isSelected
                  ? "cmux-session-index-row is-selected"
                  : "cmux-session-index-row"
              }
              aria-current={entry.isSelected ? "true" : undefined}
              onClick={() => onSelectWorkspace(entry.index)}
            >
              <span className="cmux-session-index-row-title">
                {entry.isPinned ? "Pinned · " : ""}
                {entry.title}
              </span>
              <span className="cmux-session-index-row-meta">
                {entry.panelCount} surface{entry.panelCount === 1 ? "" : "s"} ·{" "}
                {entry.layoutMode}
                {entry.hasUnread ? " · Unread" : ""}
              </span>
              {entry.directory != null && (
                <span className="cmux-session-index-row-path">{entry.directory}</span>
              )}
              {entry.description != null && (
                <span className="cmux-session-index-row-description">
                  {entry.description}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function nonBlank(value: string | null | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  return trimmed === "" ? null : trimmed;
}
