import { useEffect, useState, type CSSProperties } from "react";

import type { DoubleClickAction } from "@cmux/core-types";

import {
  fileExplorerEntryIcon,
  fileExplorerParentPath,
  fileExplorerSizeLabel,
  isMarkdownFilePath,
  listFileExplorerDirectory,
  openFileExplorerPath,
  type FileExplorerEntry,
} from "../host/fileExplorer";
import { useSession } from "../hooks/useSession";
import {
  rightSidebarModeItems,
  type RightSidebarMode,
} from "../rightSidebarModes";
import { useFocusedPanelId } from "../session/focusedPane";
import { SessionsIndexPanel } from "./SessionsIndexPanel";
import { FeedPanel } from "./FeedPanel";

export { isMarkdownFilePath } from "../host/fileExplorer";
export type { RightSidebarMode } from "../rightSidebarModes";

export interface FileExplorerPanelProps {
  open: boolean;
  mode?: RightSidebarMode;
  doubleClickAction?: DoubleClickAction;
  preferredEditor?: string;
  rightMaxWidth?: number;
  feedEnabled?: boolean;
  onModeChange?: (mode: RightSidebarMode) => void;
  onOpenFind?: () => void;
  onClose: () => void;
}

export function selectedFileExplorerEntry(
  entries: readonly FileExplorerEntry[],
  selectedRelativePath: string | null,
): FileExplorerEntry | undefined {
  if (selectedRelativePath == null) {
    return entries[0];
  }
  return (
    entries.find((entry) => entry.relativePath === selectedRelativePath) ?? entries[0]
  );
}

export function nextFileExplorerSelection(
  entries: readonly FileExplorerEntry[],
  selectedRelativePath: string | null,
  direction: -1 | 1,
): string | null {
  if (entries.length === 0) {
    return null;
  }
  const existingIndex = entries.findIndex(
    (entry) => entry.relativePath === selectedRelativePath,
  );
  const currentIndex =
    existingIndex >= 0 ? existingIndex : direction === 1 ? -1 : 0;
  const nextIndex = (currentIndex + direction + entries.length) % entries.length;
  return entries[nextIndex]?.relativePath ?? null;
}

const DEFAULT_RIGHT_SIDEBAR_WIDTH = 292;

export function rightSidebarWidthStyle(
  rightMaxWidth: number | undefined,
): CSSProperties | undefined {
  if (
    rightMaxWidth === undefined ||
    !Number.isFinite(rightMaxWidth) ||
    rightMaxWidth <= 0
  ) {
    return undefined;
  }
  const cappedWidth = Math.min(DEFAULT_RIGHT_SIDEBAR_WIDTH, rightMaxWidth);
  return {
    flexBasis: `${cappedWidth}px`,
    maxWidth: `${rightMaxWidth}px`,
  };
}

export function FileExplorerPanel({
  open,
  mode = "files",
  doubleClickAction = "preview",
  preferredEditor = "",
  rightMaxWidth,
  feedEnabled = false,
  onModeChange,
  onOpenFind,
  onClose,
}: FileExplorerPanelProps): React.JSX.Element | null {
  const {
    activeLayout,
    workspaces,
    selectedWorkspaceIndex,
    openMarkdownFile,
    openFile,
    selectWorkspace,
  } = useSession();
  const focusedPanelId = useFocusedPanelId(activeLayout);
  const workspace = workspaces[selectedWorkspaceIndex];
  const rootPath = workspace?.current_directory ?? "";
  const [relativePath, setRelativePath] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [entries, setEntries] = useState<FileExplorerEntry[]>([]);
  const [selectedRelativePath, setSelectedRelativePath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const widthStyle = rightSidebarWidthStyle(rightMaxWidth);
  const modeItems = rightSidebarModeItems({
    feedEnabled,
    dockEnabled: false,
  });

  useEffect(() => {
    if (open) {
      setRelativePath("");
      setSelectedRelativePath(null);
      setStatus(null);
    }
  }, [open, rootPath]);

  useEffect(() => {
    if (!open) {
      return;
    }
    if (rootPath.trim() === "") {
      setEntries([]);
      setStatus("No workspace directory is available.");
      return;
    }

    let disposed = false;
    setLoading(true);
    setStatus(null);
    void listFileExplorerDirectory({
      rootPath,
      relativePath,
      showHidden,
    })
      .then((next) => {
        if (!disposed) {
          setEntries(next);
          setSelectedRelativePath((current) =>
            current != null &&
            next.some((entry) => entry.relativePath === current)
              ? current
              : (next[0]?.relativePath ?? null),
          );
        }
      })
      .catch((error) => {
        if (!disposed) {
          setEntries([]);
          setStatus(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!disposed) {
          setLoading(false);
        }
      });

    return () => {
      disposed = true;
    };
  }, [open, refreshToken, relativePath, rootPath, showHidden]);

  if (!open) {
    return null;
  }

  const selectMode = (nextMode: RightSidebarMode) => {
    onModeChange?.(nextMode);
    if (nextMode === "find") {
      onOpenFind?.();
    }
  };

  const openEntry = (entry: FileExplorerEntry) => {
    if (entry.kind === "directory") {
      setRelativePath(entry.relativePath);
      return;
    }
    if (doubleClickAction === "preview" && isMarkdownFilePath(entry.path)) {
      if (focusedPanelId == null) {
        setStatus("Select a pane before opening a Markdown preview.");
        return;
      }
      openMarkdownFile(focusedPanelId, entry.path);
      setStatus(`Opened ${entry.name} in the focused pane.`);
      return;
    }
    if (doubleClickAction === "preview") {
      if (focusedPanelId == null) {
        setStatus("Select a pane before opening a file editor.");
        return;
      }
      openFile(focusedPanelId, entry.path);
      setStatus(`Opened ${entry.name} in the focused pane.`);
      return;
    }
    if (doubleClickAction === "preferredEditor") {
      setStatus(`Opening ${entry.name}...`);
      void openFileExplorerPath({
        path: entry.path,
        preferredEditor,
      })
        .then((reply) => setStatus(reply.message))
        .catch((error) =>
          setStatus(error instanceof Error ? error.message : String(error)),
        );
      return;
    }
  };

  const parentPath = fileExplorerParentPath(relativePath);
  const selectedEntry = selectedFileExplorerEntry(entries, selectedRelativePath);

  return (
    <aside
      className="cmux-file-explorer"
      aria-label="Right Sidebar"
      tabIndex={-1}
      style={widthStyle}
      onKeyDown={(event) => {
        if (mode !== "files") {
          return;
        }
        if (event.key === "ArrowDown" && (event.metaKey || event.ctrlKey)) {
          if (selectedEntry != null) {
            event.preventDefault();
            openEntry(selectedEntry);
          }
          return;
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          setSelectedRelativePath((current) =>
            nextFileExplorerSelection(entries, current, event.key === "ArrowDown" ? 1 : -1),
          );
          return;
        }
        if (
          (event.key === "Enter" || event.key === "ArrowDown") &&
          (event.key === "Enter" || event.metaKey || event.ctrlKey)
        ) {
          if (selectedEntry != null) {
            event.preventDefault();
            openEntry(selectedEntry);
          }
        }
      }}
    >
      <header className="cmux-file-explorer-header">
        <div>
          <h2>Right Sidebar</h2>
          <p>
            {mode === "sessions"
              ? "Vault"
              : mode === "feed"
                ? "Feed"
              : mode === "find"
                ? "Find in files"
                : relativePath === ""
                  ? rootPath || "No workspace"
                  : relativePath}
          </p>
        </div>
        <button type="button" aria-label="Close right sidebar" onClick={onClose}>
          Close
        </button>
      </header>
      <div
        className="cmux-right-sidebar-modebar"
        role="tablist"
        aria-label="Right sidebar mode"
      >
        {modeItems.map((item) => (
          <button
            key={item.mode}
            type="button"
            role="tab"
            className={
              item.mode === mode
                ? "cmux-right-sidebar-mode is-selected"
                : "cmux-right-sidebar-mode"
            }
            aria-selected={item.mode === mode}
            aria-label={`Show Sidebar ${item.label}`}
            onClick={() => selectMode(item.mode)}
          >
            {item.label}
          </button>
        ))}
      </div>
      {mode === "feed" ? (
        <FeedPanel />
      ) : mode === "sessions" ? (
        <SessionsIndexPanel
          workspaces={workspaces}
          selectedWorkspaceIndex={selectedWorkspaceIndex}
          onSelectWorkspace={selectWorkspace}
        />
      ) : mode === "find" ? (
        <div className="cmux-file-explorer-empty">
          <strong>Find in files</strong>
          <span>Use the search overlay to scan the current workspace directory.</span>
          <button type="button" onClick={onOpenFind}>
            Open Find
          </button>
        </div>
      ) : (
        <>
          <div className="cmux-file-explorer-toolbar">
            <button
              type="button"
              disabled={parentPath == null}
              onClick={() => setRelativePath(parentPath ?? "")}
            >
              Up
            </button>
            <button type="button" onClick={() => setRefreshToken((token) => token + 1)}>
              Refresh
            </button>
            <button
              type="button"
              disabled={selectedEntry == null}
              onClick={() => selectedEntry != null && openEntry(selectedEntry)}
            >
              Open
            </button>
            <label>
              <input
                type="checkbox"
                checked={showHidden}
                onChange={() => setShowHidden((current) => !current)}
              />
              Hidden
            </label>
          </div>
          {status != null && <p className="cmux-file-explorer-status">{status}</p>}
          {loading ? (
            <div className="cmux-file-explorer-empty">Loading files...</div>
          ) : entries.length === 0 ? (
            <div className="cmux-file-explorer-empty">No files to show.</div>
          ) : (
            <ul className="cmux-file-explorer-list">
              {entries.map((entry) => (
                <li key={entry.relativePath}>
                  <button
                    type="button"
                    className={
                      entry.relativePath === selectedRelativePath
                        ? "cmux-file-explorer-row is-selected"
                        : "cmux-file-explorer-row"
                    }
                    aria-label={`${
                      entry.kind === "directory" ? "Open folder" : "Open file"
                    } ${entry.name}`}
                    aria-current={
                      entry.relativePath === selectedRelativePath ? "true" : undefined
                    }
                    onDoubleClick={() => openEntry(entry)}
                    onClick={() => {
                      setSelectedRelativePath(entry.relativePath);
                      setStatus(null);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        event.stopPropagation();
                        openEntry(entry);
                        return;
                      }
                      if (
                        event.key === "ArrowDown" &&
                        (event.metaKey || event.ctrlKey)
                      ) {
                        event.preventDefault();
                        event.stopPropagation();
                        openEntry(entry);
                        return;
                      }
                      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                        event.preventDefault();
                        event.stopPropagation();
                        setSelectedRelativePath((current) =>
                          nextFileExplorerSelection(
                            entries,
                            current,
                            event.key === "ArrowDown" ? 1 : -1,
                          ),
                        );
                        return;
                      }
                    }}
                  >
                    <span className="cmux-file-explorer-kind">
                      {fileExplorerEntryIcon(entry.kind)}
                    </span>
                    <span className="cmux-file-explorer-name">{entry.name}</span>
                    <span className="cmux-file-explorer-size">
                      {fileExplorerSizeLabel(entry.size)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </aside>
  );
}
