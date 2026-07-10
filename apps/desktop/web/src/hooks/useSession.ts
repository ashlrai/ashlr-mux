import { useCallback, useEffect, useState } from "react";

import type {
  AppSessionSnapshot,
  SessionSplitOrientation,
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceLayoutSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { host } from "../host/host";
import { activeLayoutOf } from "../session/activeLayout";
import { focusedPaneStore } from "../session/focusedPane";
import type { SwitchableSurfaceKind } from "../session/surfaceUrl";
import type { SplitPath } from "../session/splitLayout";

export interface NewWorkspaceOptions {
  currentDirectory?: string;
  initialTerminalCommand?: string;
  initialTerminalInput?: string;
  initialTerminalEnvironment?: Record<string, string>;
}

export interface SplitOptions {
  initialTerminalCommand?: string;
  initialTerminalInput?: string;
  initialTerminalEnvironment?: Record<string, string>;
}

export type CanvasAction =
  | "tidy"
  | "alignLeft"
  | "alignRight"
  | "alignTop"
  | "alignBottom"
  | "equalizeWidths"
  | "equalizeHeights"
  | "distributeHorizontally"
  | "distributeVertically";

/**
 * React binding for the Rust session layer (`src-tauri/src/session.rs`).
 *
 * On mount it pulls the authoritative snapshot (`session_snapshot`) and
 * subscribes to `cmux://session-changed`, so every structural mutation — from
 * this window or any other trigger — flows back through one channel. The mutator
 * commands (`session_split` / `session_close` / `session_set_divider`) also
 * return the fresh snapshot; we apply that immediately AND rely on the event, so
 * the two paths reconcile to the same value.
 *
 * Command argument keys are snake_case to match the `#[tauri::command]`
 * signatures (`panel_id`, `position`).
 */
export interface UseSession {
  /** The whole app snapshot, or `null` until the first fetch resolves. */
  snapshot: AppSessionSnapshot | null;
  /** The layout tree of the selected workspace of the first window. */
  activeLayout: SessionWorkspaceLayoutSnapshot | null;
  /** The first window's workspaces (the sidebar / tab list), in order. */
  workspaces: readonly SessionWorkspaceSnapshot[];
  /** The first window's workspace groups, if any. */
  workspaceGroups?: readonly SessionWorkspaceGroupSnapshot[];
  /** Index of the selected workspace in `workspaces` (clamped, defaults to 0). */
  selectedWorkspaceIndex: number;
  /** Create a fresh terminal workspace and select it. */
  newWorkspace: (currentDirectoryOrOptions?: string | NewWorkspaceOptions) => void;
  /** Select the workspace at `index`. */
  selectWorkspace: (index: number) => void;
  /** Select a workspace by id and focus a panel/tab inside it. */
  selectWorkspaceSurface: (workspaceId: string, panelId: string) => void;
  /** Close the workspace at `index` (always leaves at least one alive). */
  closeWorkspace: (index: number) => void;
  /**
   * Close multiple workspaces addressed by their ORIGINAL indices in the
   * current workspace list. The session layer canonicalizes them into current
   * tab order, so callers can pass above/below/other ranges without racing
   * index shifts.
   */
  closeWorkspaces: (indices: readonly number[]) => void;
  /**
   * Reorder the workspace at raw `index` toward `toIndex`. `usesTopLevelRows`
   * enables the canonical grouped-child promotion path for sidebar drags.
   */
  reorderWorkspace: (
    index: number,
    toIndex: number,
    usesTopLevelRows?: boolean,
  ) => void;
  /**
   * Rename the workspace at `index`; empty/whitespace-only clears the custom
   * title, restoring the process-title fallback (canonical `setCustomTitle`).
   */
  renameWorkspace: (index: number, title: string) => void;
  /**
   * Set or clear the workspace description at `index`; blank/whitespace-only
   * clears it after canonical line-ending normalization.
   */
  setWorkspaceDescription: (index: number, description: string) => void;
  /** Clear the workspace's custom tab color override. */
  resetWorkspaceColor: (index: number) => void;
  /** Set or clear the selected workspace's unread marker. */
  setWorkspaceUnread: (
    index: number,
    unread: boolean,
    preferredPanelId?: string,
  ) => void;
  /**
   * Set or clear the custom title for a panel/tab. Empty or whitespace-only
   * clears the custom title.
   */
  renameTab: (panelId: string, title: string) => void;
  /** Set or clear the pinned state for a panel/tab. */
  setPanelPinned: (panelId: string, pinned: boolean) => void;
  /** Set or clear the unread marker for a panel/tab. */
  setPanelUnread: (panelId: string, unread: boolean) => void;
  /**
   * Pin/unpin the workspace at `index`; pinned rows float to the top tier
   * (canonical setPinned).
   */
  setWorkspacePinned: (index: number, pinned: boolean) => void;
  /** Set the collapsed state of workspace group `groupId`. */
  setGroupCollapsed: (groupId: string, collapsed: boolean) => void;
  /**
   * Split the pane holding `panelId` in `orientation`. `insertFirst` puts the
   * new pane in the first (left/top) slot — the canonical left/up direction;
   * omitted = second (right/down).
   */
  split: (
    panelId: string,
    orientation: SessionSplitOrientation,
    insertFirst?: boolean,
    options?: SplitOptions,
  ) => void;
  /** Add a new terminal tab in the pane containing `panelId`. */
  newTerminalTab: (panelId: string, options?: SplitOptions) => void;
  /** Equalize every divider in the active workspace (span-count semantics). */
  equalizeDividers: () => void;
  /** Toggle split zoom for the pane containing `panelId`. */
  toggleSplitZoom: (panelId: string) => void;
  /** Set the active workspace layout mode (`"canvas"` or split/default). */
  setLayoutMode: (mode: "canvas" | null) => void;
  /** Toggle the active workspace between split layout and canvas layout. */
  toggleCanvasLayout: () => void;
  /** Persist a canvas pane frame for the active workspace. */
  setCanvasPaneFrame: (
    panelId: string,
    frame: { x: number; y: number; width: number; height: number },
  ) => void;
  /** Apply a canvas command to the active workspace's persisted pane frames. */
  applyCanvasAction: (action: CanvasAction, options?: { paneGap?: number }) => void;
  /** Close the pane/panel `panelId`. */
  close: (panelId: string) => void;
  /** Persist the divider ratio of the split reached by `path`. */
  setDivider: (path: SplitPath, position: number) => void;
  /**
   * Set the non-terminal surface kind of the pane holding `panelId`, or `null`
   * to revert it to a terminal.
   */
  setSurfaceKind: (panelId: string, kind: SwitchableSurfaceKind | null) => void;
  /** Select the next/previous tab hosted by the pane that contains `panelId`. */
  selectAdjacentPanel: (panelId: string, next: boolean) => void;
  /** Open `filePath` in the pane's markdown viewer and switch that pane to it. */
  openMarkdownFile: (panelId: string, filePath: string) => void;
  /** Open `filePath` in the pane's file editor and switch that pane to it. */
  openFile: (panelId: string, filePath: string) => void;
  /** Open a registered diff session in the pane's diff viewer. */
  openDiffViewer: (panelId: string, token: string, requestPath?: string) => void;
  /** Open `url` in the pane's browser and switch that pane to it. */
  openBrowserUrl: (panelId: string, url?: string) => void;
  /** Navigate the pane browser back through persisted history. */
  browserBack: (panelId: string) => void;
  /** Navigate the pane browser forward through persisted history. */
  browserForward: (panelId: string) => void;
  /** Clear persisted back/forward browser history for the pane. */
  clearBrowserHistory: (panelId: string) => void;
  /** Clear retained browser Network inspector records for the pane. */
  clearBrowserNetworkRecords: (panelId: string) => void;
  /** Toggle the pane browser omnibar/toolbar visibility. */
  toggleBrowserOmnibar: (panelId: string) => void;
  /** Toggle browser focus mode for the pane. */
  toggleBrowserFocusMode: (panelId: string) => void;
  /** Toggle the pane browser developer-tools drawer. */
  toggleBrowserDeveloperTools: (panelId: string) => void;
  /** Show the pane browser developer-tools drawer on a named panel. */
  showBrowserDeveloperTools: (
    panelId: string,
    panel: "inspector" | "console" | "react" | "network",
  ) => void;
  /** Create a browser workspace and select it. */
  newBrowserWorkspace: (url?: string) => void;
  /** Reopen the most recently closed browser surface into a new workspace. */
  reopenClosedBrowserTab: () => void;
  /** Move an existing panel/surface into a newly-created workspace. */
  movePanelToNewWorkspace: (panelId: string) => void;
  /** Split a pane and make the new pane a browser. */
  splitBrowser: (
    panelId: string,
    orientation: SessionSplitOrientation,
    insertFirst?: boolean,
    url?: string,
  ) => void;
  /** Persist a pane browser zoom factor. */
  setBrowserZoom: (panelId: string, zoom: number) => void;
}

export function useSession(): UseSession {
  const [snapshot, setSnapshot] = useState<AppSessionSnapshot | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void host
      .invoke<AppSessionSnapshot>("session_snapshot")
      .then((next) => {
        if (!disposed) {
          setSnapshot(next);
        }
      })
      .catch(() => {});

    void host
      .on<AppSessionSnapshot>("cmux://session-changed", (next) => {
        if (!disposed) {
          setSnapshot(next);
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // NOTE: Tauri v2 maps JS camelCase argument keys onto Rust snake_case command
  // params, so the pane id must be sent as `panelId` (→ `panel_id`), NOT
  // `panel_id`. `path`/`position`/`orientation` are single words, unaffected.
  const split = useCallback(
    (
      panelId: string,
      orientation: SessionSplitOrientation,
      insertFirst?: boolean,
      options?: SplitOptions,
    ) => {
      void host
        .invoke<AppSessionSnapshot>("session_split", {
          panelId,
          orientation,
          insertFirst,
          initialTerminalCommand: options?.initialTerminalCommand,
          initialTerminalInput: options?.initialTerminalInput,
          initialTerminalEnvironment: options?.initialTerminalEnvironment,
        })
        .then(setSnapshot)
        .catch((error) => console.error("session_split failed", error));
    },
    [],
  );

  const splitBrowser = useCallback(
    (
      panelId: string,
      orientation: SessionSplitOrientation,
      insertFirst?: boolean,
      url?: string,
    ) => {
      void host
        .invoke<AppSessionSnapshot>("session_split_browser", {
          panelId,
          orientation,
          insertFirst,
          url,
        })
        .then(setSnapshot)
        .catch((error) => console.error("session_split_browser failed", error));
    },
    [],
  );

  const newTerminalTab = useCallback((panelId: string, options?: SplitOptions) => {
    void host
      .invoke<AppSessionSnapshot>("session_new_terminal_tab", {
        panelId,
        initialTerminalCommand: options?.initialTerminalCommand,
        initialTerminalInput: options?.initialTerminalInput,
        initialTerminalEnvironment: options?.initialTerminalEnvironment,
      })
      .then(setSnapshot)
      .catch((error) => console.error("session_new_terminal_tab failed", error));
  }, []);

  const equalizeDividers = useCallback(() => {
    void host
      .invoke<AppSessionSnapshot>("session_equalize_dividers")
      .then(setSnapshot)
      .catch((error) => console.error("session_equalize_dividers failed", error));
  }, []);

  const toggleSplitZoom = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_toggle_split_zoom", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_toggle_split_zoom failed", error));
  }, []);

  const setLayoutMode = useCallback((mode: "canvas" | null) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_layout_mode", { mode })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_layout_mode failed", error));
  }, []);

  const setCanvasPaneFrame = useCallback(
    (
      panelId: string,
      frame: { x: number; y: number; width: number; height: number },
    ) => {
      void host
        .invoke<AppSessionSnapshot>("session_set_canvas_pane_frame", {
          panelId,
          x: Math.round(frame.x),
          y: Math.round(frame.y),
          width: Math.round(frame.width),
          height: Math.round(frame.height),
        })
        .then(setSnapshot)
        .catch((error) =>
          console.error("session_set_canvas_pane_frame failed", error),
        );
    },
    [],
  );

  const applyCanvasAction = useCallback((action: CanvasAction, options?: { paneGap?: number }) => {
    void host
      .invoke<AppSessionSnapshot>("session_apply_canvas_action", {
        action,
        paneGap:
          options?.paneGap === undefined ? undefined : Math.round(options.paneGap),
      })
      .then(setSnapshot)
      .catch((error) => console.error("session_apply_canvas_action failed", error));
  }, []);

  const close = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_close", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_close failed", error));
  }, []);

  const setDivider = useCallback((path: SplitPath, position: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_divider", { path, position })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_divider failed", error));
  }, []);

  const setSurfaceKind = useCallback(
    (panelId: string, kind: SwitchableSurfaceKind | null) => {
      void host
        .invoke<AppSessionSnapshot>("session_set_surface_kind", { panelId, kind })
        .then(setSnapshot)
        .catch((error) => console.error("session_set_surface_kind failed", error));
    },
    [],
  );

  const selectAdjacentPanel = useCallback((panelId: string, next: boolean) => {
    void host
      .invoke<AppSessionSnapshot>("session_select_adjacent_panel", { panelId, next })
      .then((snapshot) => {
        setSnapshot(snapshot);
        const selectedPanelId = selectedPanelInPaneContaining(
          activeLayoutOf(snapshot),
          panelId,
        );
        if (selectedPanelId !== undefined) {
          focusedPaneStore.focus(selectedPanelId);
        }
      })
      .catch((error) => console.error("session_select_adjacent_panel failed", error));
  }, []);

  const selectWorkspaceSurface = useCallback((workspaceId: string, panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_select_workspace_surface", {
        workspaceId,
        panelId,
      })
      .then((snapshot) => {
        setSnapshot(snapshot);
        focusedPaneStore.focus(panelId);
      })
      .catch((error) => console.error("session_select_workspace_surface failed", error));
  }, []);

  const newWorkspace = useCallback((currentDirectoryOrOptions?: string | NewWorkspaceOptions) => {
    const options =
      typeof currentDirectoryOrOptions === "string"
        ? { currentDirectory: currentDirectoryOrOptions }
        : currentDirectoryOrOptions;
    void host
      .invoke<AppSessionSnapshot>("session_new_workspace", {
        currentDirectory: options?.currentDirectory,
        initialTerminalCommand: options?.initialTerminalCommand,
        initialTerminalInput: options?.initialTerminalInput,
        initialTerminalEnvironment: options?.initialTerminalEnvironment,
      })
      .then(setSnapshot)
      .catch((error) => console.error("session_new_workspace failed", error));
  }, []);

  const newBrowserWorkspace = useCallback((url?: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_new_browser_workspace", { url })
      .then(setSnapshot)
      .catch((error) => console.error("session_new_browser_workspace failed", error));
  }, []);

  const reopenClosedBrowserTab = useCallback(() => {
    void host
      .invoke<AppSessionSnapshot>("session_reopen_closed_browser_tab")
      .then(setSnapshot)
      .catch((error) =>
        console.error("session_reopen_closed_browser_tab failed", error),
      );
  }, []);

  const movePanelToNewWorkspace = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_move_panel_to_new_workspace", { panelId })
      .then(setSnapshot)
      .catch((error) =>
        console.error("session_move_panel_to_new_workspace failed", error),
      );
  }, []);

  const openMarkdownFile = useCallback((panelId: string, filePath: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_open_markdown_file", { panelId, filePath })
      .then(setSnapshot)
      .catch((error) => console.error("session_open_markdown_file failed", error));
  }, []);

  const openFile = useCallback((panelId: string, filePath: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_open_file", { panelId, filePath })
      .then(setSnapshot)
      .catch((error) => console.error("session_open_file failed", error));
  }, []);

  const openDiffViewer = useCallback((panelId: string, token: string, requestPath?: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_open_diff_viewer", {
        panelId,
        token,
        requestPath,
      })
      .then(setSnapshot)
      .catch((error) => console.error("session_open_diff_viewer failed", error));
  }, []);

  const openBrowserUrl = useCallback((panelId: string, url?: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_open_browser_url", { panelId, url })
      .then(setSnapshot)
      .catch((error) => console.error("session_open_browser_url failed", error));
  }, []);

  const browserBack = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_browser_go_back", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_browser_go_back failed", error));
  }, []);

  const browserForward = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_browser_go_forward", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_browser_go_forward failed", error));
  }, []);

  const clearBrowserHistory = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_clear_browser_history", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_clear_browser_history failed", error));
  }, []);

  const clearBrowserNetworkRecords = useCallback((panelId: string) => {
    void host
      .invoke("browser_clear_network_requests", { panelId })
      .catch((error) => console.error("browser_clear_network_requests failed", error));
  }, []);

  const toggleBrowserOmnibar = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_toggle_browser_omnibar", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_toggle_browser_omnibar failed", error));
  }, []);

  const toggleBrowserFocusMode = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_toggle_browser_focus_mode", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_toggle_browser_focus_mode failed", error));
  }, []);

  const toggleBrowserDeveloperTools = useCallback((panelId: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_toggle_browser_developer_tools", { panelId })
      .then(setSnapshot)
      .catch((error) => console.error("session_toggle_browser_developer_tools failed", error));
  }, []);

  const showBrowserDeveloperTools = useCallback(
    (panelId: string, panel: "inspector" | "console" | "react" | "network") => {
      void host
        .invoke<AppSessionSnapshot>("session_show_browser_developer_tools", {
          panelId,
          panel,
        })
        .then(setSnapshot)
        .catch((error) => console.error("session_show_browser_developer_tools failed", error));
    },
    [],
  );

  const setBrowserZoom = useCallback((panelId: string, zoom: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_browser_zoom", { panelId, zoom })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_browser_zoom failed", error));
  }, []);

  const selectWorkspace = useCallback((index: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_select_workspace", { index })
      .then(setSnapshot)
      .catch((error) => console.error("session_select_workspace failed", error));
  }, []);

  const closeWorkspace = useCallback((index: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_close_workspace", { index })
      .then(setSnapshot)
      .catch((error) => console.error("session_close_workspace failed", error));
  }, []);

  const closeWorkspaces = useCallback((indices: readonly number[]) => {
    void host
      .invoke<AppSessionSnapshot>("session_close_workspaces", { indices: [...indices] })
      .then(setSnapshot)
      .catch((error) => console.error("session_close_workspaces failed", error));
  }, []);

  const reorderWorkspace = useCallback(
    (index: number, toIndex: number, usesTopLevelRows?: boolean) => {
      void host
        .invoke<AppSessionSnapshot>("session_reorder_workspaces", {
          index,
          toIndex,
          usesTopLevelRows,
        })
        .then(setSnapshot)
        .catch((error) => console.error("session_reorder_workspaces failed", error));
    },
    [],
  );

  const renameWorkspace = useCallback((index: number, title: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_rename_workspace", { index, title })
      .then(setSnapshot)
      .catch((error) => console.error("session_rename_workspace failed", error));
  }, []);

  const setWorkspaceDescription = useCallback((index: number, description: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_workspace_description", { index, description })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_workspace_description failed", error));
  }, []);

  const resetWorkspaceColor = useCallback((index: number) => {
    void host
      .invoke<AppSessionSnapshot>("session_reset_workspace_color", { index })
      .then(setSnapshot)
      .catch((error) => console.error("session_reset_workspace_color failed", error));
  }, []);

  const setWorkspaceUnread = useCallback(
    (index: number, unread: boolean, preferredPanelId?: string) => {
      void host
        .invoke<AppSessionSnapshot>("session_set_workspace_unread", {
          index,
          unread,
          preferredPanelId,
        })
        .then(setSnapshot)
        .catch((error) => console.error("session_set_workspace_unread failed", error));
    },
    [],
  );

  const renameTab = useCallback((panelId: string, title: string) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_panel_title", { panelId, title })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_panel_title failed", error));
  }, []);

  const setPanelPinned = useCallback((panelId: string, pinned: boolean) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_panel_pinned", { panelId, pinned })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_panel_pinned failed", error));
  }, []);

  const setPanelUnread = useCallback((panelId: string, unread: boolean) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_panel_unread", { panelId, unread })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_panel_unread failed", error));
  }, []);

  // Both arg keys are single words — no camelCase → snake_case mapping hazard.
  const setWorkspacePinned = useCallback((index: number, pinned: boolean) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_workspace_pinned", { index, pinned })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_workspace_pinned failed", error));
  }, []);

  const setGroupCollapsed = useCallback((groupId: string, collapsed: boolean) => {
    void host
      .invoke<AppSessionSnapshot>("session_set_group_collapsed", { groupId, collapsed })
      .then(setSnapshot)
      .catch((error) => console.error("session_set_group_collapsed failed", error));
  }, []);

  const tabs = snapshot?.windows[0]?.tab_manager;
  const workspaces = tabs?.workspaces ?? [];
  const rawIndex = tabs?.selected_workspace_index ?? 0;
  const selectedWorkspaceIndex =
    rawIndex >= 0 && rawIndex < workspaces.length ? rawIndex : 0;
  const toggleCanvasLayout = useCallback(() => {
    const selected = workspaces[selectedWorkspaceIndex];
    setLayoutMode(selected?.layout_mode === "canvas" ? null : "canvas");
  }, [selectedWorkspaceIndex, setLayoutMode, workspaces]);

  return {
    snapshot,
    activeLayout: activeLayoutOf(snapshot),
    workspaces,
    workspaceGroups: tabs?.workspace_groups,
    selectedWorkspaceIndex,
    split,
    newTerminalTab,
    equalizeDividers,
    toggleSplitZoom,
    setLayoutMode,
    toggleCanvasLayout,
    setCanvasPaneFrame,
    applyCanvasAction,
    close,
    setDivider,
    setSurfaceKind,
    selectAdjacentPanel,
    openMarkdownFile,
    openFile,
    openDiffViewer,
    openBrowserUrl,
    browserBack,
    browserForward,
    clearBrowserHistory,
    clearBrowserNetworkRecords,
    toggleBrowserOmnibar,
    toggleBrowserFocusMode,
    toggleBrowserDeveloperTools,
    showBrowserDeveloperTools,
    newBrowserWorkspace,
    reopenClosedBrowserTab,
    movePanelToNewWorkspace,
    splitBrowser,
    setBrowserZoom,
    newWorkspace,
    selectWorkspace,
    closeWorkspace,
    closeWorkspaces,
    reorderWorkspace,
    renameWorkspace,
    setWorkspaceDescription,
    resetWorkspaceColor,
    setWorkspaceUnread,
    renameTab,
    setPanelPinned,
    setPanelUnread,
    setWorkspacePinned,
    setGroupCollapsed,
    selectWorkspaceSurface,
  };
}

function selectedPanelInPaneContaining(
  layout: SessionWorkspaceLayoutSnapshot | null,
  panelId: string,
): string | undefined {
  if (layout === null) {
    return undefined;
  }
  if (layout.type === "pane") {
    return layout.pane.panel_ids.includes(panelId) ||
      layout.pane.selected_panel_id === panelId
      ? (layout.pane.selected_panel_id ?? layout.pane.panel_ids[0])
      : undefined;
  }
  return (
    selectedPanelInPaneContaining(layout.split.first, panelId) ??
    selectedPanelInPaneContaining(layout.split.second, panelId)
  );
}
