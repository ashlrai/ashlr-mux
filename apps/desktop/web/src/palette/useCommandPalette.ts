import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  CommandPaletteCommand,
} from "../components/CommandRow";
import {
  dispatchBrowserCommand,
  dispatchBrowserNetworkCleared,
} from "../components/BrowserSurface";
import { dispatchTerminalCommand } from "../components/TerminalSurface";
import { dispatchPanelFlash, dispatchPanelFlashSequence } from "../components/Workspace";
import type { CommandPaletteResolvedSearchMatch } from "../components/CommandPalette";
import type { Config, SessionWorkspaceSnapshot } from "@cmux/core-types";

import { createDiffSession } from "../host/diffViewer";
import { host } from "../host/host";
import type { RightSidebarMode } from "../rightSidebarModes";
import {
  resetMarkdownZoom,
  zoomMarkdownIn,
  zoomMarkdownOut,
} from "../host/markdownBridge";
import { useSession } from "../hooks/useSession";
import {
  buildCommandCatalog,
  dispatchCommand,
  searchableTexts,
  type CommandContext,
  type CommandContribution,
  type BuildCommandCatalogOptions,
  type CanvasCommandAction,
} from "./commandCatalog";
import {
  COMMAND_PALETTE_COMMANDS_PREFIX,
  type CommandPaletteListScope,
} from "./listScope";
import {
  buildSwitcherEntries,
  switcherCandidateCommandIds,
  workspaceDisplayName,
  type SwitcherEntry,
} from "./switcherEntries";
import {
  adjacentPanelId,
  adjacentCanvasPanelId,
  focusedPaneStore,
  paneIdForPanelId,
  panelIdsInLayout,
  useFocusedPanelId,
} from "../session/focusedPane";
import { countLeaves, type Layout } from "../session/splitLayout";
import { hasExecutablePlan, planIntent, type IntentPlanContext } from "./intentPlan";
import { surfaceKinds } from "../session/paneRects";
import {
  applyMove,
  applyQueryChange,
  applyResults,
  initialQueryState,
} from "./paletteQuery";
import { RenderSequencingGuard } from "./renderSequencing";
import { buildRightSidebarModeContributions } from "./rightSidebarModeContributions";
import { buildSettingsToggleContributions } from "./settingsToggleContributions";
import {
  browserUsageHistoryStorage,
  readCommandPaletteUsageHistory,
  recordCommandPaletteUsage,
  writeCommandPaletteUsageHistory,
  type CommandPaletteUsageHistory,
} from "./usageHistory";
import type { ConfigAction } from "../settings/configReducer";
import type { RightSidebarModeAvailability } from "../rightSidebarModes";
import type { SettingsPaneSection } from "../settings/settingsSearchResults";
import {
  WARM_CLAUDE_CODE_SHORTCUT_ACTION,
  shortcutActionForEvent,
} from "../settings/shortcutRuntime";

/**
 * D4 — the live command-palette host. Composes the ported pure modules
 * (`commandCatalog`, `switcherEntries`, `listScope`) and the Rust search bridge
 * (`command_palette_search`, the D2 orchestrator) into an openable overlay, and
 * emits the exact `commands` + `matches` shapes the pure {@link CommandPalette}
 * list renderer already consumes.
 *
 * Scope mirrors canonical cmux: an empty query (or any query NOT starting with
 * `>`) is the **switcher** (fuzzy workspace jump); a `>`-prefixed query is the
 * **commands** list. `queryForMatching` strips the prefix before matching.
 *
 * Search runs through the Rust orchestrator when Tauri is present (the parity
 * path); in a plain-browser dev runtime it falls back to a client-side filter so
 * the overlay is still visible and navigable.
 */

/** One corpus record, matching the Rust `CorpusEntryInput` (camelCase). */
interface CorpusEntry {
  commandId: string;
  rank: number;
  title: string;
  searchableTexts: string[];
}

/** One resolved match from the Rust bridge (`SearchMatch`, camelCase). */
interface SearchMatch {
  commandId: string;
  score: number;
  titleMatchIndices: number[];
}

const canvasToggleContribution: CommandContribution = {
  commandId: "palette.canvas.toggleLayout",
  title: (ctx) =>
    ctx.workspaceCanvasLayout === true
      ? "Disable Canvas Layout"
      : "Enable Canvas Layout",
  subtitle: () => "Canvas",
  keywords: ["canvas", "layout", "freeform", "panes", "workspace"],
  dismissOnRun: true,
  when: (ctx) => ctx.hasWorkspace === true,
  enablement: (ctx) => ctx.hasWorkspace === true,
  intent: { kind: "toggleCanvasLayout" },
};

const CANVAS_ACTION_CONTRIBUTIONS: ReadonlyArray<{
  commandId: string;
  title: string;
  action: CanvasCommandAction;
  keywords: readonly string[];
}> = [
  {
    commandId: "palette.canvas.tidy",
    title: "Tidy Canvas",
    action: "tidy",
    keywords: ["pack", "arrange", "layout"],
  },
  {
    commandId: "palette.canvas.alignLeft",
    title: "Align Canvas Panes Left",
    action: "alignLeft",
    keywords: ["align", "left", "edge"],
  },
  {
    commandId: "palette.canvas.alignRight",
    title: "Align Canvas Panes Right",
    action: "alignRight",
    keywords: ["align", "right", "edge"],
  },
  {
    commandId: "palette.canvas.alignTop",
    title: "Align Canvas Panes Top",
    action: "alignTop",
    keywords: ["align", "top", "edge"],
  },
  {
    commandId: "palette.canvas.alignBottom",
    title: "Align Canvas Panes Bottom",
    action: "alignBottom",
    keywords: ["align", "bottom", "edge"],
  },
  {
    commandId: "palette.canvas.equalizeWidths",
    title: "Equalize Canvas Pane Widths",
    action: "equalizeWidths",
    keywords: ["same", "width", "size"],
  },
  {
    commandId: "palette.canvas.equalizeHeights",
    title: "Equalize Canvas Pane Heights",
    action: "equalizeHeights",
    keywords: ["same", "height", "size"],
  },
  {
    commandId: "palette.canvas.distributeHorizontally",
    title: "Distribute Canvas Panes Horizontally",
    action: "distributeHorizontally",
    keywords: ["space", "horizontal", "gap"],
  },
  {
    commandId: "palette.canvas.distributeVertically",
    title: "Distribute Canvas Panes Vertically",
    action: "distributeVertically",
    keywords: ["space", "vertical", "gap"],
  },
];

const canvasActionContributions: CommandContribution[] =
  CANVAS_ACTION_CONTRIBUTIONS.map((contribution) => ({
    commandId: contribution.commandId,
    title: () => contribution.title,
    subtitle: () => "Canvas",
    keywords: ["canvas", ...contribution.keywords],
    dismissOnRun: true,
    when: (ctx) => ctx.workspaceCanvasLayout === true,
    enablement: (ctx) => ctx.workspaceCanvasLayout === true,
    intent: { kind: "canvasAction", action: contribution.action },
  }));

export interface UseCommandPalette {
  visible: boolean;
  query: string;
  scope: CommandPaletteListScope;
  /** Commands available for the current scope, keyed by `id` for the list. */
  commands: CommandPaletteCommand[];
  /** Resolved matches (ordered), in the list renderer's snake_case shape. */
  matches: CommandPaletteResolvedSearchMatch[];
  selectedIndex: number;
  open: (initialQuery?: string) => void;
  close: () => void;
  setQuery: (query: string) => void;
  move: (delta: number) => void;
  hoverAt: (index: number) => void;
  activateAt: (index: number) => void;
  editor: CommandPaletteEditorState | null;
  setEditorDraft: (draft: string) => void;
  submitEditor: () => void;
  cancelEditor: () => void;
}

export type CommandPaletteEditorState =
  | {
      kind: "renameWorkspace";
      workspaceId: string;
      title: string;
      draft: string;
    }
  | {
      kind: "renameTab";
      panelId: string;
      title: string;
      draft: string;
    }
  | {
      kind: "workspaceDescription";
      workspaceId: string;
      title: string;
      draft: string;
    };

const RESULT_LIMIT = 50;

/** Client-side fallback matcher used only when the Tauri bridge is absent. */
function fallbackSearch(
  corpus: CorpusEntry[],
  matchingQuery: string,
  resultLimit: number,
): SearchMatch[] {
  const query = matchingQuery.toLowerCase().trim();
  if (query === "") {
    return corpus
      .slice(0, resultLimit)
      .map((entry) => ({ commandId: entry.commandId, score: 0, titleMatchIndices: [] }));
  }
  const matches: SearchMatch[] = [];
  for (const entry of corpus) {
    const haystack = entry.searchableTexts.join(" ").toLowerCase();
    if (!haystack.includes(query)) {
      continue;
    }
    const titleIdx = entry.title.toLowerCase().indexOf(query);
    const titleMatchIndices =
      titleIdx >= 0 ? Array.from({ length: query.length }, (_, i) => titleIdx + i) : [];
    matches.push({
      commandId: entry.commandId,
      score: titleIdx >= 0 ? 100 : 50,
      titleMatchIndices,
    });
  }
  return matches.slice(0, resultLimit);
}

interface ActiveBrowserPaletteState {
  omnibarVisible: boolean;
  focusModeActive: boolean;
}

interface ActiveControlRefs {
  surfaceRef?: string;
  paneRef?: string;
}

function browserPaletteStateForPanelId(
  layout: Layout | null,
  panelId: string | undefined,
): ActiveBrowserPaletteState | undefined {
  if (layout === null || panelId === undefined) {
    return undefined;
  }
  if (layout.type === "pane") {
    if (!layout.pane.panel_ids.includes(panelId)) {
      return undefined;
    }
    return {
      omnibarVisible: layout.pane.browser_omnibar_visible ?? true,
      focusModeActive: layout.pane.browser_focus_mode_active ?? false,
    };
  }
  return (
    browserPaletteStateForPanelId(layout.split.first, panelId) ??
    browserPaletteStateForPanelId(layout.split.second, panelId)
  );
}

function controlRefsForPanelId(
  layout: Layout | null,
  panelId: string | undefined,
): ActiveControlRefs {
  if (layout === null || panelId === undefined) {
    return {};
  }
  let surfaceIndex = 0;
  const walk = (node: Layout): ActiveControlRefs | undefined => {
    if (node.type === "pane") {
      for (const candidatePanelId of node.pane.panel_ids) {
        const currentIndex = surfaceIndex;
        surfaceIndex += 1;
        if (candidatePanelId !== panelId) {
          continue;
        }
        const refs: ActiveControlRefs = {
          surfaceRef: `surface:${currentIndex + 1}`,
        };
        if (node.pane.pane_id !== undefined) {
          refs.paneRef = `pane:${currentIndex + 1}`;
        }
        return refs;
      }
      return undefined;
    }
    return walk(node.split.first) ?? walk(node.split.second);
  };
  return walk(layout) ?? {};
}

interface UnreadTarget {
  workspaceIndex: number;
  panelId?: string;
}

interface MarkUnreadAndJumpNextResult {
  marked: UnreadTarget | null;
  next: UnreadTarget | null;
}

interface CliInstallStatus {
  installed_in_path: boolean;
}

interface DefaultTerminalStatus {
  is_default: boolean;
}

export function nextUnreadTarget(
  workspaces: readonly SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex: number,
): UnreadTarget | null {
  if (workspaces.length === 0) {
    return null;
  }
  const normalizedStart =
    selectedWorkspaceIndex >= 0 && selectedWorkspaceIndex < workspaces.length
      ? selectedWorkspaceIndex
      : 0;
  for (let offset = 0; offset < workspaces.length; offset += 1) {
    const index = (normalizedStart + offset) % workspaces.length;
    const workspace = workspaces[index];
    const unread = workspace.panel_unreads?.find((entry) => entry.is_unread);
    if (unread === undefined) {
      continue;
    }
    if (workspace.layout === null || workspace.layout === undefined) {
      return { workspaceIndex: index, panelId: unread.panel_id };
    }
    const livePanelIds = panelIdsInLayout(workspace.layout);
    const liveUnread = workspace.panel_unreads?.find(
      (entry) => entry.is_unread && livePanelIds.has(entry.panel_id),
    );
    return {
      workspaceIndex: index,
      panelId: liveUnread?.panel_id ?? unread.panel_id,
    };
  }
  return null;
}

function unreadEntryTargetInWorkspace(
  workspace: SessionWorkspaceSnapshot,
  workspaceIndex: number,
  panelId: string,
): UnreadTarget | null {
  if (workspace.layout === null || workspace.layout === undefined) {
    return { workspaceIndex, panelId };
  }
  const livePanelIds = panelIdsInLayout(workspace.layout);
  if (livePanelIds.has(panelId)) {
    return { workspaceIndex, panelId };
  }
  const liveUnread = workspace.panel_unreads?.find(
    (entry) => entry.is_unread && livePanelIds.has(entry.panel_id),
  );
  return {
    workspaceIndex,
    panelId: liveUnread?.panel_id ?? panelId,
  };
}

export function oldestUnreadTarget(
  workspaces: readonly SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex: number,
): UnreadTarget | null {
  const unreadEntries = workspaces.flatMap((workspace, workspaceIndex) =>
    (workspace.panel_unreads ?? [])
      .filter((entry) => entry.is_unread)
      .map((entry) => ({ workspace, workspaceIndex, entry })),
  );
  if (unreadEntries.length === 0) {
    return null;
  }
  if (unreadEntries.some(({ entry }) => entry.unread_at === undefined)) {
    return nextUnreadTarget(workspaces, selectedWorkspaceIndex);
  }
  const oldest = unreadEntries.reduce((best, candidate) =>
    (candidate.entry.unread_at ?? 0) < (best.entry.unread_at ?? 0)
      ? candidate
      : best,
  );
  return unreadEntryTargetInWorkspace(
    oldest.workspace,
    oldest.workspaceIndex,
    oldest.entry.panel_id,
  );
}

export function markUnreadAndJumpNextTarget(
  workspaces: readonly SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex: number,
): MarkUnreadAndJumpNextResult {
  const marked = oldestUnreadTarget(workspaces, selectedWorkspaceIndex);
  if (marked === null) {
    return { marked: null, next: null };
  }
  const afterMarkRead = workspaces.map((workspace, index) =>
    index === marked.workspaceIndex
      ? { ...workspace, panel_unreads: undefined }
      : workspace,
  );
  return {
    marked,
    next: nextUnreadTarget(afterMarkRead, marked.workspaceIndex),
  };
}

/** Host actions the palette cannot reach through the session layer. */
export interface CommandPaletteHostActions {
  /** Collapse/expand the workspace sidebar (App-owned view state). */
  toggleSidebar?: () => void;
  /** Show/hide the workspace file explorer (App-owned right sidebar state). */
  toggleFileExplorer?: () => void;
  /** Open the right sidebar to a specific canonical mode. */
  setRightSidebarMode?: (mode: RightSidebarMode) => void;
  rightSidebarModeAvailability?: RightSidebarModeAvailability;
  /** Open the Settings surface (App-owned modal state). */
  openSettings?: (options?: { section?: SettingsPaneSection; query?: string }) => void;
  /** Open the session-backed notification drawer. */
  openNotifications?: () => void;
  /** Open the selected-workspace directory search drawer. */
  openFindInDirectory?: () => void;
  /** Open a new native desktop window. */
  newWindow?: () => void;
  /** Close the current desktop window. */
  closeWindow?: () => void;
  /** Toggle native fullscreen for the current desktop window. */
  toggleFullScreen?: () => void;
  /** Latest loaded settings snapshot for runtime settings-toggle rows. */
  settingsConfig?: Config | null;
  /** Persist a settings mutation triggered from the palette. */
  applyConfigAction?: (action: ConfigAction) => void;
}

export function useCommandPalette(
  hostActions?: CommandPaletteHostActions,
): UseCommandPalette {
  const {
    snapshot,
    activeLayout,
    workspaces,
    workspaceGroups,
    selectedWorkspaceIndex,
    selectWorkspace,
    selectWorkspaceSurface,
    newWorkspace,
    newTerminalTab,
    closeWorkspace,
    closeWorkspaces,
    reorderWorkspace,
    split,
    equalizeDividers,
    toggleSplitZoom,
    setLayoutMode,
    applyCanvasAction,
    renameWorkspace,
    setWorkspaceDescription,
    resetWorkspaceColor,
    setWorkspaceUnread,
    renameTab,
    setPanelPinned,
    setPanelUnread,
    setWorkspacePinned,
    setGroupCollapsed,
    selectAdjacentPanel,
    setSurfaceKind,
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
  } = useSession();

  const [visible, setVisible] = useState(false);
  const [queryState, setQueryState] = useState(() => initialQueryState("", 0));
  const [matches, setMatches] = useState<CommandPaletteResolvedSearchMatch[]>([]);
  const [editor, setEditor] = useState<CommandPaletteEditorState | null>(null);
  const [usageHistory, setUsageHistory] = useState<CommandPaletteUsageHistory>(() =>
    readCommandPaletteUsageHistory(browserUsageHistoryStorage()),
  );
  const [vscodeInlineOpenTargetAvailable, setVSCodeInlineOpenTargetAvailable] =
    useState(false);
  const [cliInstalledInPATH, setCLIInstalledInPATH] = useState(false);
  const [defaultTerminalIsDefault, setDefaultTerminalIsDefault] = useState(false);
  const [activeCallbackScheme, setActiveCallbackScheme] = useState<string | undefined>(
    undefined,
  );
  const renderSequencing = useRef(new RenderSequencingGuard());
  const resultsVersion = useRef(0);

  const { query, scope, matchingQuery, selection } = queryState;
  const selectedIndex = selection.index;

  useEffect(() => {
    let cancelled = false;
    void host
      .invoke<string>("active_callback_scheme")
      .then((scheme) => {
        if (!cancelled) {
          setActiveCallbackScheme(scheme);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setActiveCallbackScheme(undefined);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void host
      .invoke<boolean>("vscode_inline_open_target_available")
      .then((available) => {
        if (!cancelled) {
          setVSCodeInlineOpenTargetAvailable(available);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setVSCodeInlineOpenTargetAvailable(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void host
      .invoke<DefaultTerminalStatus>("default_terminal_status")
      .then((status) => {
        if (!cancelled) {
          setDefaultTerminalIsDefault(status.is_default);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDefaultTerminalIsDefault(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void host
      .invoke<CliInstallStatus>("cli_install_status")
      .then((status) => {
        if (!cancelled) {
          setCLIInstalledInPATH(status.installed_in_path);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setCLIInstalledInPATH(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Focused-panel id (≡ the canonical surface id): the tracked focused pane
  // (pointer-down/focus capture on Workspace's pane wrappers), revalidated
  // against the active layout with a first-leaf fallback. It feeds BOTH the
  // catalog context (`hasFocusedPanel` row gating) and the intent planner. It
  // must be identical in both, or `dispatchCommand` inside `activateAt` rebuilds
  // the catalog without a row the list displayed and activation silently nulls.
  const activePanelId = useFocusedPanelId(activeLayout);
  const activePaneId = useMemo(
    () =>
      activeLayout !== null && activePanelId !== undefined
        ? paneIdForPanelId(activeLayout, activePanelId)
        : undefined,
    [activeLayout, activePanelId],
  );
  const activePanelKind = useMemo(
    () =>
      activeLayout !== null && activePanelId !== undefined
        ? surfaceKinds(activeLayout).get(activePanelId)
        : undefined,
    [activeLayout, activePanelId],
  );
  const activeBrowserPaletteState = useMemo(
    () => browserPaletteStateForPanelId(activeLayout, activePanelId),
    [activeLayout, activePanelId],
  );
  const activeControlRefs = useMemo(
    () => controlRefsForPanelId(activeLayout, activePanelId),
    [activeLayout, activePanelId],
  );
  const adjacentPanelIds = useMemo(() => {
    const selectedWorkspace = workspaces[selectedWorkspaceIndex];
    if (selectedWorkspace?.layout_mode === "canvas") {
      return {
        left: adjacentCanvasPanelId(
          selectedWorkspace.canvas_panes,
          activePanelId,
          "left",
        ),
        right: adjacentCanvasPanelId(
          selectedWorkspace.canvas_panes,
          activePanelId,
          "right",
        ),
        up: adjacentCanvasPanelId(
          selectedWorkspace.canvas_panes,
          activePanelId,
          "up",
        ),
        down: adjacentCanvasPanelId(
          selectedWorkspace.canvas_panes,
          activePanelId,
          "down",
        ),
      };
    }
    return {
      left: adjacentPanelId(activeLayout, activePanelId, "left"),
      right: adjacentPanelId(activeLayout, activePanelId, "right"),
      up: adjacentPanelId(activeLayout, activePanelId, "up"),
      down: adjacentPanelId(activeLayout, activePanelId, "down"),
    };
  }, [activeLayout, activePanelId, selectedWorkspaceIndex, workspaces]);
  const activePanelCustomTitle = useMemo(() => {
    if (activePanelId === undefined) {
      return undefined;
    }
    return workspaces[selectedWorkspaceIndex]?.panel_titles?.find(
      (entry) => entry.panel_id === activePanelId,
    )?.custom_title;
  }, [activePanelId, selectedWorkspaceIndex, workspaces]);
  const activePanelDisplayTitle = activePanelCustomTitle ?? "Tab";
  const activePanelIsPinned = useMemo(() => {
    if (activePanelId === undefined) {
      return false;
    }
    return workspaces[selectedWorkspaceIndex]?.panel_pins?.some(
      (entry) => entry.panel_id === activePanelId && entry.is_pinned,
    ) ?? false;
  }, [activePanelId, selectedWorkspaceIndex, workspaces]);
  const activePanelHasUnread = useMemo(() => {
    if (activePanelId === undefined) {
      return false;
    }
    return workspaces[selectedWorkspaceIndex]?.panel_unreads?.some(
      (entry) => entry.panel_id === activePanelId && entry.is_unread,
    ) ?? false;
  }, [activePanelId, selectedWorkspaceIndex, workspaces]);
  const activeForkableAgentSnapshot = useMemo(() => {
    if (activePanelId === undefined) {
      return undefined;
    }
    return workspaces[selectedWorkspaceIndex]?.restorable_agent_snapshots?.find(
      (entry) => entry.panel_id === activePanelId,
    )?.snapshot;
  }, [activePanelId, selectedWorkspaceIndex, workspaces]);
  const activePanelHasForkableAgent =
    Boolean(activeForkableAgentSnapshot?.fork_command?.trim());
  const selectedWorkspaceHasUnread = useMemo(() => {
    return workspaces[selectedWorkspaceIndex]?.panel_unreads?.some(
      (entry) => entry.is_unread,
    ) ?? false;
  }, [selectedWorkspaceIndex, workspaces]);
  const activeWorkspacePanelCount = useMemo(
    () => (activeLayout === null ? 0 : panelIdsInLayout(activeLayout).size),
    [activeLayout],
  );

  // The ONE catalog context, shared by the catalog build below and the
  // `dispatchCommand` call inside `activateAt`. The two MUST be identical
  // field-for-field: `dispatchCommand` rebuilds the catalog, so any ctx drift
  // makes a displayed row's `when` gate drop it at activation time and the
  // click silently no-ops. Keys mirror the canonical host's computation off
  // tabManager.selectedWorkspace (ContentView.swift:6154-6161):
  // - workspaceHasCustomName ≡ customTitle != nil (:6159) — both sides hold
  //   the trimmed-nonempty-or-absent invariant via A6, so presence == has
  //   custom name.
  // - workspaceShouldPin ≡ !workspace.isPinned for the live selected
  //   workspace (:6161); snapshot is_pinned is Some(true)|None, so undefined
  //   reads as unpinned.
  const commandContext = useMemo<CommandContext>(() => {
    const selected = workspaces[selectedWorkspaceIndex];
    const selectedGroup = (workspaceGroups ?? []).find(
      (group) => group.id === selected?.group_id,
    );
    const browser = hostActions?.settingsConfig?.browser;
    const browserDisabled =
      browser !== undefined && browser !== null
        ? !browser.openTerminalLinksInCmuxBrowser &&
          !browser.interceptTerminalOpenCommandInCmuxBrowser
        : false;
    const workspaceMinimalModeEnabled =
      hostActions?.settingsConfig?.app?.minimalMode ?? false;
    const sidebarMatchTerminalBackground =
      hostActions?.settingsConfig?.sidebar_appearance?.matchTerminalBackground ??
      false;
    return {
      hasWorkspace: workspaces.length > 0,
      workspaceName: selected?.custom_title ?? selected?.process_title ?? null,
      workspaceHasCustomDescription:
        selected?.custom_description !== undefined &&
        selected.custom_description.trim() !== "",
      workspaceHasAbove: selected !== undefined && selectedWorkspaceIndex > 0,
      workspaceHasBelow:
        selected !== undefined && selectedWorkspaceIndex < workspaces.length - 1,
      workspaceHasPeers: workspaces.length > 1,
      workspaceHasSplits:
        activeLayout !== null && countLeaves(activeLayout) > 1,
      workspaceCanMoveUp: selected !== undefined && selectedWorkspaceIndex > 0,
      workspaceCanMoveDown:
        selected !== undefined && selectedWorkspaceIndex < workspaces.length - 1,
      workspaceCanMarkRead: selectedWorkspaceHasUnread,
      workspaceCanMarkUnread: selected !== undefined && !selectedWorkspaceHasUnread,
      workspaceGroupCanCollapse:
        selectedGroup !== undefined && selectedGroup.is_collapsed !== true,
      workspaceGroupCanExpand:
        selectedGroup !== undefined && selectedGroup.is_collapsed === true,
      workspaceHasPullRequests:
        selected?.current_directory !== undefined &&
        selected.current_directory.trim() !== "",
      workspaceMinimalModeEnabled,
      sidebarMatchTerminalBackground,
      hasFocusedPanel: activePanelId !== undefined,
      panelHasPane: activePaneId !== undefined,
      panelHasCustomName: activePanelCustomTitle !== undefined,
      panelShouldPin: !activePanelIsPinned,
      panelHasUnread: activePanelHasUnread,
      panelHasForkableAgent: activePanelHasForkableAgent,
      panelName: activePanelCustomTitle ?? null,
      panelCanMoveToNewWorkspace:
        activePanelId !== undefined && activeWorkspacePanelCount > 1,
      panelIsMarkdown: activePanelKind === "markdown",
      panelIsBrowser: activePanelKind === "browser",
      panelIsTerminal: activePanelKind === "terminal" || activePanelKind === undefined,
      panelBrowserFocusModeActive: activeBrowserPaletteState?.focusModeActive,
      panelBrowserOmnibarVisible: activeBrowserPaletteState?.omnibarVisible,
      browserDisabled,
      cliInstalledInPATH,
      defaultTerminalIsDefault,
      workspaceHasCustomName: selected?.custom_title !== undefined,
      workspaceShouldPin: !(selected?.is_pinned === true),
      workspaceCanvasLayout: selected?.layout_mode === "canvas",
      vscodeInlineOpenTargetAvailable,
    };
  }, [
    workspaces,
    workspaceGroups,
    selectedWorkspaceIndex,
    activeLayout,
    activePanelId,
    activePaneId,
    activePanelCustomTitle,
    activePanelIsPinned,
    activePanelHasUnread,
    activePanelHasForkableAgent,
    selectedWorkspaceHasUnread,
    activeWorkspacePanelCount,
    activePanelKind,
    activeBrowserPaletteState,
    hostActions?.settingsConfig?.browser,
    hostActions?.settingsConfig?.app?.minimalMode,
    hostActions?.settingsConfig?.sidebar_appearance?.matchTerminalBackground,
    vscodeInlineOpenTargetAvailable,
    cliInstalledInPATH,
    defaultTerminalIsDefault,
  ]);

  const catalogOptions = useMemo<BuildCommandCatalogOptions>(
    () => ({
      dynamicContributions: {
        rightSidebarMode:
          hostActions?.setRightSidebarMode === undefined
            ? []
            : buildRightSidebarModeContributions(
                hostActions.rightSidebarModeAvailability,
              ),
        canvas: [canvasToggleContribution, ...canvasActionContributions],
        settingsToggle: buildSettingsToggleContributions(
          hostActions?.settingsConfig,
        ),
      },
    }),
    [
      hostActions?.rightSidebarModeAvailability,
      hostActions?.setRightSidebarMode,
      hostActions?.settingsConfig,
    ],
  );

  const intentPlanContext = useMemo<IntentPlanContext>(() => {
    const selectedWorkspace = workspaces[selectedWorkspaceIndex];
    const selectedGroup = (workspaceGroups ?? []).find(
      (group) => group.id === selectedWorkspace?.group_id,
    );
    return {
      selectedWorkspaceIndex,
      workspaceCount: workspaces.length,
      activePanelId,
      activeSurfaceRef: activeControlRefs.surfaceRef,
      activePaneId,
      activePaneRef: activeControlRefs.paneRef,
      navigationScheme: activeCallbackScheme,
      selectedWorkspaceId: selectedWorkspace?.workspace_id,
      selectedWorkspaceTitle:
        selectedWorkspace !== undefined
          ? workspaceDisplayName(selectedWorkspace)
          : undefined,
      activePanelTitle: activePanelDisplayTitle,
      activePanelIsPinned,
      activePanelHasUnread,
      selectedWorkspaceDescription: selectedWorkspace?.custom_description ?? "",
      selectedWorkspaceIsPinned: selectedWorkspace?.is_pinned === true,
      selectedWorkspaceIsUnread: selectedWorkspaceHasUnread,
      selectedWorkspaceLayoutMode: selectedWorkspace?.layout_mode,
      selectedWorkspaceGroupId: selectedGroup?.id,
      selectedWorkspaceGroupIsCollapsed: selectedGroup?.is_collapsed,
      activeForkCommand: activeForkableAgentSnapshot?.fork_command,
      activeForkWorkingDirectory:
        activeForkableAgentSnapshot?.working_directory ?? selectedWorkspace?.current_directory,
      selectedWorkspaceCurrentDirectory: selectedWorkspace?.current_directory,
      adjacentPanelIds,
    };
  }, [
    workspaces,
    workspaceGroups,
    selectedWorkspaceIndex,
    activePanelId,
    activeControlRefs,
    activePaneId,
    activePanelDisplayTitle,
    activePanelIsPinned,
    activePanelHasUnread,
    activeCallbackScheme,
    adjacentPanelIds,
    activeForkableAgentSnapshot,
    selectedWorkspaceHasUnread,
  ]);

  // Build the scope's commands, the search corpus, the switcher candidate ids,
  // and (switcher) the id→workspace map for activation. The pure modules do all
  // the parity work; this only marshals their output.
  const { commands, corpus, candidateCommandIds, switcherById } = useMemo(() => {
    if (scope === "commands") {
      const built: CommandPaletteCommand[] = buildCommandCatalog(
        commandContext,
        catalogOptions,
      )
        .filter((descriptor) => {
          const dispatch = dispatchCommand(
            descriptor.id,
            commandContext,
            catalogOptions,
          );
          if (dispatch === null) {
            return false;
          }
          if (dispatch.intent.kind === "toggleSetting") {
            return hostActions?.applyConfigAction !== undefined;
          }
          if (dispatch.intent.kind === "canvasAction") {
            return commandContext.workspaceCanvasLayout === true;
          }
          if (dispatch.intent.kind === "rightSidebarMode") {
            return hostActions?.setRightSidebarMode !== undefined;
          }
          return hasExecutablePlan(
            planIntent(dispatch.intent.kind, intentPlanContext),
          );
        })
        .map((d) => ({
          id: d.id,
          rank: d.rank,
          title: d.title,
          subtitle: d.subtitle,
          shortcut_hint: d.shortcutHint ?? null,
          kind_label: d.kindLabel,
          keywords: d.keywords,
          dismiss_on_run: d.dismissOnRun,
        }));
      return {
        commands: built,
        corpus: toCorpus(built),
        candidateCommandIds: [] as string[],
        switcherById: new Map<string, SwitcherEntry>(),
      };
    }

    // switcher scope
    const tabs = snapshot?.windows[0]?.tab_manager ?? {
      workspaces: [],
      selected_workspace_index: 0,
    };
    const entries = buildSwitcherEntries(tabs, { includeSurfaces: true });
    const built: CommandPaletteCommand[] = entries.map((entry) => ({
      id: entry.id,
      rank: entry.rank,
      title: entry.title,
      subtitle: entry.subtitle,
      shortcut_hint: null,
      kind_label: entry.kindLabel,
      keywords: entry.keywords,
      dismiss_on_run: entry.dismissOnRun,
    }));
    const byId = new Map(entries.map((entry) => [entry.id, entry]));
    return {
      commands: built,
      corpus: toCorpus(built),
      candidateCommandIds: switcherCandidateCommandIds(entries),
      switcherById: byId,
    };
  }, [
    scope,
    snapshot,
    commandContext,
    catalogOptions,
    hostActions?.applyConfigAction,
    intentPlanContext,
  ]);

  // Run the search whenever the query, scope, corpus, or visibility changes.
  useEffect(() => {
    if (!visible) {
      return;
    }
    const sequence = renderSequencing.current.issue();
    const version = (resultsVersion.current += 1);
    let cancelled = false;

    void host
      .invoke<SearchMatch[]>("command_palette_search", {
        request: {
          scope,
          query: matchingQuery,
          candidateCommandIds,
          corpus,
          usageHistory,
          queryIsEmpty: matchingQuery === "",
          historyTimestamp: Date.now() / 1000,
          resultLimit: RESULT_LIMIT,
        },
      })
      .catch(() => fallbackSearch(corpus, matchingQuery, RESULT_LIMIT))
      .then((found) => {
        if (
          cancelled ||
          !renderSequencing.current.applyIfCurrent(sequence, version)
        ) {
          return;
        }
        const resolved: CommandPaletteResolvedSearchMatch[] = found.map((m) => ({
          command_id: m.commandId,
          score: m.score,
          title_match_indices: m.titleMatchIndices,
        }));
        setMatches(resolved);
        setQueryState((prev) => applyResults(prev, resolved.length));
      });

    return () => {
      cancelled = true;
    };
  }, [
    visible,
    scope,
    matchingQuery,
    corpus,
    candidateCommandIds,
    commands.length,
    usageHistory,
  ]);

  const open = useCallback((initialQuery = "") => {
    setQueryState(initialQueryState(initialQuery, 0));
    setMatches([]);
    setEditor(null);
    setVisible(true);
  }, []);

  const close = useCallback(() => {
    setVisible(false);
    setQueryState(initialQueryState("", 0));
    setMatches([]);
    setEditor(null);
  }, []);

  const setQuery = useCallback((next: string) => {
    setQueryState((prev) => {
      const result = applyQueryChange(prev, next);
      if (result.resetVisibleResults) {
        setMatches([]);
      }
      return result.state;
    });
  }, []);

  const move = useCallback(
    (delta: number) => {
      if (delta === 0) {
        return;
      }
      setQueryState((prev) => {
        let next = prev;
        const direction = delta < 0 ? "up" : "down";
        for (let index = 0; index < Math.abs(delta); index += 1) {
          next = applyMove(next, direction);
        }
        return next;
      });
    },
    [],
  );

  const hoverAt = useCallback((index: number) => {
    setQueryState((prev) => ({
      ...prev,
      selection: {
        index:
          prev.selection.count <= 0
            ? 0
            : Math.max(0, Math.min(index, prev.selection.count - 1)),
        count: prev.selection.count,
      },
    }));
  }, []);

  const resolveWorkspaceIndex = useCallback(
    (workspaceId: string) =>
      workspaces.findIndex((workspace) => workspace.workspace_id === workspaceId),
    [workspaces],
  );

  const setEditorDraft = useCallback((draft: string) => {
    setEditor((current) => (current ? { ...current, draft } : current));
  }, []);

  const cancelEditor = useCallback(() => {
    setEditor(null);
  }, []);

  const submitEditor = useCallback(() => {
    if (editor == null) {
      return;
    }
    if (editor.kind === "renameTab") {
      renameTab(editor.panelId, editor.draft);
      close();
      return;
    }
    const index = resolveWorkspaceIndex(editor.workspaceId);
    if (index < 0) {
      setEditor(null);
      return;
    }
    if (editor.kind === "renameWorkspace") {
      renameWorkspace(index, editor.draft);
    } else {
      setWorkspaceDescription(index, editor.draft);
    }
    close();
  }, [
    close,
    editor,
    renameWorkspace,
    renameTab,
    resolveWorkspaceIndex,
    setWorkspaceDescription,
  ]);

  const warmClaudeCode = useCallback((currentDirectory: string | undefined) => {
    const params =
      currentDirectory !== undefined && currentDirectory.trim() !== ""
        ? { workingDirectory: currentDirectory }
        : {};
    void host
      .invoke("agent_session_rpc", {
        message: {
          method: "provider.warmClaude",
          params,
        },
      })
      .catch((error) => {
        console.error("provider.warmClaude failed", error);
      });
  }, []);

  const activateAt = useCallback(
    (index: number) => {
      const match = matches[index];
      if (!match) {
        return;
      }
      const recordUsage = (): void => {
        const timestamp = Date.now() / 1000;
        setUsageHistory((current) => {
          const next = recordCommandPaletteUsage(
            current,
            match.command_id,
            timestamp,
          );
          writeCommandPaletteUsageHistory(browserUsageHistoryStorage(), next);
          return next;
        });
      };
      if (scope === "switcher") {
        const entry = switcherById.get(match.command_id);
        if (entry) {
          recordUsage();
          if (entry.target.panelId !== undefined) {
            selectWorkspaceSurface(entry.target.workspaceId, entry.target.panelId);
          } else {
            const targetIndex = workspaces.findIndex(
              (w) => w.workspace_id === entry.target.workspaceId,
            );
            if (targetIndex >= 0) {
              selectWorkspace(targetIndex);
            }
          }
        }
        close();
      } else {
        // Commands scope: resolve the intent through the shared dispatch path
        // (with the SAME ctx the catalog was built from — see commandContext),
        // decide what it does with the pure planner, and execute. Unmapped
        // kinds still log so the wiring never silently no-ops.
        const dispatch = dispatchCommand(
          match.command_id,
          commandContext,
          catalogOptions,
        );
        if (dispatch) {
          recordUsage();
          if (dispatch.intent.kind === "toggleSetting") {
            hostActions?.applyConfigAction?.(dispatch.intent.action);
            if (dispatch.dismissOnRun) {
              close();
            }
            return;
          }
          if (dispatch.intent.kind === "canvasAction") {
            applyCanvasAction(dispatch.intent.action, {
              paneGap: hostActions?.settingsConfig?.canvas?.paneGap,
            });
            if (dispatch.dismissOnRun) {
              close();
            }
            return;
          }
          if (dispatch.intent.kind === "rightSidebarMode") {
            hostActions?.setRightSidebarMode?.(dispatch.intent.mode);
            if (dispatch.dismissOnRun) {
              close();
            }
            return;
          }
          const selectedWorkspace = workspaces[selectedWorkspaceIndex];
          const plan = planIntent(dispatch.intent.kind, intentPlanContext);
          switch (plan.type) {
            case "newWorkspace":
              newWorkspace();
              break;
            case "newBrowserWorkspace":
              newBrowserWorkspace();
              break;
            case "reopenClosedBrowserTab":
              reopenClosedBrowserTab();
              break;
            case "installCLI":
              void host
                .invoke<CliInstallStatus>("install_cli")
                .then((status) => {
                  setCLIInstalledInPATH(status.installed_in_path);
                })
                .catch((error) => {
                  console.error("install_cli failed", error);
                });
              break;
            case "uninstallCLI":
              void host
                .invoke<CliInstallStatus>("uninstall_cli")
                .then((status) => {
                  setCLIInstalledInPATH(status.installed_in_path);
                })
                .catch((error) => {
                  console.error("uninstall_cli failed", error);
                });
              break;
            case "makeDefaultTerminal":
              void host
                .invoke<DefaultTerminalStatus>("make_default_terminal")
                .then((status) => {
                  setDefaultTerminalIsDefault(status.is_default);
                })
                .catch((error) => {
                  console.error("make_default_terminal failed", error);
                });
              break;
            case "warmClaudeCode": {
              warmClaudeCode(plan.currentDirectory);
              break;
            }
            case "closeWorkspace":
              closeWorkspace(plan.index);
              break;
            case "closeWorkspaces":
              closeWorkspaces(plan.indexes);
              break;
            case "selectWorkspace":
              selectWorkspace(plan.index);
              break;
            case "jumpUnread": {
              const target = nextUnreadTarget(workspaces, selectedWorkspaceIndex);
              if (target !== null) {
                if (target.panelId !== undefined) {
                  focusedPaneStore.focus(target.panelId);
                }
                selectWorkspace(target.workspaceIndex);
              }
              break;
            }
            case "markOldestUnreadAndJumpNext": {
              const target = markUnreadAndJumpNextTarget(
                workspaces,
                selectedWorkspaceIndex,
              );
              if (target.marked !== null) {
                setWorkspaceUnread(
                  target.marked.workspaceIndex,
                  false,
                  target.marked.panelId,
                );
              }
              if (target.next !== null) {
                if (target.next.panelId !== undefined) {
                  focusedPaneStore.focus(target.next.panelId);
                }
                selectWorkspace(target.next.workspaceIndex);
              }
              break;
            }
            case "split":
              split(plan.panelId, plan.orientation, plan.insertFirst, {
                initialTerminalInput: plan.initialTerminalInput,
              });
              break;
            case "forkAgentNewWorkspace":
              newWorkspace({
                currentDirectory: plan.currentDirectory,
                initialTerminalInput: plan.initialTerminalInput,
              });
              break;
            case "forkAgentNewTab":
              newTerminalTab(plan.panelId, {
                initialTerminalInput: plan.initialTerminalInput,
              });
              break;
            case "splitBrowser":
              splitBrowser(plan.panelId, plan.orientation, plan.insertFirst);
              break;
            case "equalizeDividers":
              equalizeDividers();
              break;
            case "toggleSplitZoom":
              toggleSplitZoom(plan.panelId);
              break;
            case "setLayoutMode":
              setLayoutMode(plan.mode);
              break;
            case "triggerPanelFlash":
              dispatchPanelFlash(plan.panelId);
              break;
            case "setWorkspacePinned":
              setWorkspacePinned(plan.index, plan.pinned);
              break;
            case "setWorkspaceGroupCollapsed":
              setGroupCollapsed(plan.groupId, plan.collapsed);
              break;
            case "setWorkspaceUnread":
              setWorkspaceUnread(plan.index, plan.unread, plan.preferredPanelId);
              break;
            case "resetWorkspaceColor":
              resetWorkspaceColor(plan.index);
              break;
            case "setSurfaceKind":
              if (plan.kind === "diff") {
                void createDiffSession()
                  .then((session) => {
                    openDiffViewer(plan.panelId, session.token, session.requestPath);
                  })
                  .catch((error) => {
                    console.error("diff_create_session failed", error);
                  });
              } else {
                setSurfaceKind(plan.panelId, plan.kind);
              }
              break;
            case "openBrowser":
              openBrowserUrl(plan.panelId);
              break;
            case "browserCommand":
              if (plan.command === "back") {
                browserBack(plan.panelId);
              } else if (plan.command === "forward") {
                browserForward(plan.panelId);
              } else {
                dispatchBrowserCommand(plan.panelId, plan.command);
              }
              break;
            case "terminalCommand":
              dispatchTerminalCommand(plan.panelId, plan.command);
              break;
            case "clearBrowserHistory":
              dispatchPanelFlashSequence(plan.panelId);
              clearBrowserHistory(plan.panelId);
              break;
            case "clearBrowserNetworkRecords":
              clearBrowserNetworkRecords(plan.panelId);
              dispatchBrowserNetworkCleared(plan.panelId);
              showBrowserDeveloperTools(plan.panelId, "network");
              break;
            case "toggleBrowserOmnibar":
              toggleBrowserOmnibar(plan.panelId);
              break;
            case "toggleBrowserFocusMode":
              toggleBrowserFocusMode(plan.panelId);
              break;
            case "toggleBrowserDeveloperTools":
              toggleBrowserDeveloperTools(plan.panelId);
              break;
            case "showBrowserDeveloperTools":
              showBrowserDeveloperTools(plan.panelId, plan.panel);
              break;
            case "reorderWorkspace":
              reorderWorkspace(plan.index, plan.toIndex, plan.usesTopLevelRows);
              break;
            case "beginRenameWorkspace":
              setEditor({
                kind: "renameWorkspace",
                workspaceId: plan.workspaceId,
                title: plan.title,
                draft: plan.title,
              });
              break;
            case "renameWorkspace":
              renameWorkspace(plan.index, plan.title);
              break;
            case "beginRenameTab":
              setEditor({
                kind: "renameTab",
                panelId: plan.panelId,
                title: plan.title,
                draft: plan.title,
              });
              break;
            case "renameTab":
              renameTab(plan.panelId, plan.title);
              break;
            case "setPanelPinned":
              setPanelPinned(plan.panelId, plan.pinned);
              break;
            case "setPanelUnread":
              setPanelUnread(plan.panelId, plan.unread);
              break;
            case "movePanelToNewWorkspace":
              movePanelToNewWorkspace(plan.panelId);
              break;
            case "beginWorkspaceDescriptionEdit":
              setEditor({
                kind: "workspaceDescription",
                workspaceId: plan.workspaceId,
                title: plan.title,
                draft: plan.description,
              });
              break;
            case "setWorkspaceDescription":
              setWorkspaceDescription(plan.index, plan.description);
              break;
            case "selectAdjacentPanel":
              selectAdjacentPanel(plan.panelId, plan.next);
              break;
            case "focusPanel":
              focusedPaneStore.focus(plan.panelId);
              dispatchPanelFlash(plan.panelId);
              break;
            case "toggleSidebar":
              hostActions?.toggleSidebar?.();
              break;
            case "toggleFileExplorer":
              hostActions?.toggleFileExplorer?.();
              break;
            case "setMinimalMode":
              hostActions?.applyConfigAction?.({
                type: "setMinimalMode",
                enabled: plan.enabled,
              });
              break;
            case "toggleMatchTerminalBackground":
              hostActions?.applyConfigAction?.({
                type: "toggleMatchTerminalBackground",
              });
              break;
            case "setBrowserEnabled":
              hostActions?.applyConfigAction?.({
                type: "setBrowserEnabled",
                enabled: plan.enabled,
              });
              break;
            case "openSettings":
              hostActions?.openSettings?.({
                section: plan.section,
                query: plan.query,
              });
              break;
            case "showNotifications":
              hostActions?.openNotifications?.();
              break;
            case "openFindInDirectory":
              hostActions?.openFindInDirectory?.();
              break;
            case "openFolderInVSCodeInline":
              void host
                .invoke<string | null>("open_folder_in_vscode_inline")
                .then((url) => {
                  if (url) {
                    newBrowserWorkspace(url);
                  }
                })
                .catch((error) => {
                  console.error("open_folder_in_vscode_inline failed", error);
                });
              break;
            case "vscodeServeWebStop":
              void host.invoke("vscode_serve_web_stop").catch((error) => {
                console.error("vscode_serve_web_stop failed", error);
              });
              break;
            case "vscodeServeWebRestart":
              void host
                .invoke<string | null>("vscode_serve_web_restart")
                .then((url) => {
                  if (url) {
                    newBrowserWorkspace(url);
                  }
                })
                .catch((error) => {
                  console.error("vscode_serve_web_restart failed", error);
                });
              break;
            case "openWorkspacePullRequests":
              void host
                .invoke<string[]>("workspace_pull_request_links", {
                  directory: selectedWorkspace?.current_directory ?? "",
                })
                .then((urls) => {
                  for (const url of urls) {
                    newBrowserWorkspace(url);
                  }
                })
                .catch((error) => {
                  console.error("workspace_pull_request_links failed", error);
                });
              break;
            case "restartControlSocketListener":
              void host.invoke("restart_control_socket_listener").catch((error) => {
                console.error("restart_control_socket_listener failed", error);
              });
              break;
            case "openCmuxSettingsFile":
              void host.invoke("open_cmux_settings_file").catch((error) => {
                console.error("open_cmux_settings_file failed", error);
              });
              break;
            case "openGhosttySettingsFile":
              void host.invoke("open_ghostty_settings_file").catch((error) => {
                console.error("open_ghostty_settings_file failed", error);
              });
              break;
            case "openFolder":
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
              break;
            case "restorePreviousLaunch":
              void host
                .invoke("session_restore_previous_launch")
                .catch((error) => {
                  console.error("session_restore_previous_launch failed", error);
                });
              break;
            case "newWindow":
              hostActions?.newWindow?.();
              break;
            case "closeWindow":
              hostActions?.closeWindow?.();
              break;
            case "toggleFullScreen":
              hostActions?.toggleFullScreen?.();
              break;
            case "openTaskManager":
              void host.invoke("window_open_task_manager").catch((error) => {
                console.error("window_open_task_manager failed", error);
              });
              break;
            case "markdownZoomIn":
              void zoomMarkdownIn(plan.panelId);
              break;
            case "markdownZoomOut":
              void zoomMarkdownOut(plan.panelId);
              break;
            case "markdownZoomReset":
              void resetMarkdownZoom(plan.panelId);
              break;
            case "copyText":
              void copyTextToClipboard(plan.text);
              break;
            case "none":
              break;
            case "unhandled":
              // eslint-disable-next-line no-console
              console.info(
                "[command-palette] unhandled intent",
                match.command_id,
                dispatch.intent.kind,
              );
              break;
          }
          if (dispatch.dismissOnRun) {
            close();
          }
        }
      }
    },
    [
      matches,
      scope,
      switcherById,
      workspaces,
      selectedWorkspaceIndex,
      activePanelId,
      activePaneId,
      intentPlanContext,
      commandContext,
      catalogOptions,
      selectWorkspace,
      selectWorkspaceSurface,
      newWorkspace,
      newTerminalTab,
      closeWorkspace,
      closeWorkspaces,
      reorderWorkspace,
      split,
      equalizeDividers,
      toggleSplitZoom,
      setLayoutMode,
      applyCanvasAction,
      setWorkspacePinned,
      setWorkspaceUnread,
      setPanelPinned,
      setPanelUnread,
      movePanelToNewWorkspace,
      resetWorkspaceColor,
      setSurfaceKind,
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
      splitBrowser,
      setWorkspaceDescription,
      renameWorkspace,
      renameTab,
      selectAdjacentPanel,
      hostActions,
      close,
      setEditor,
      warmClaudeCode,
    ],
  );

  // Global open shortcuts (only while hidden — the overlay owns key handling
  // once visible). Ctrl/Cmd+K → switcher; Ctrl/Cmd+Shift+P → commands.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (visible) {
        return;
      }
      const action = shortcutActionForEvent(
        hostActions?.settingsConfig?.shortcuts?.bindings,
        event,
      );
      if (action === WARM_CLAUDE_CODE_SHORTCUT_ACTION) {
        if (isEditableShortcutTarget(event.target)) {
          return;
        }
        const plan = planIntent("warmClaudeCode", intentPlanContext);
        if (plan.type === "warmClaudeCode") {
          event.preventDefault();
          warmClaudeCode(plan.currentDirectory);
        }
        return;
      }
      if (!(event.metaKey || event.ctrlKey)) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        open("");
      } else if (key === "p" && event.shiftKey) {
        event.preventDefault();
        open(COMMAND_PALETTE_COMMANDS_PREFIX);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    hostActions?.settingsConfig?.shortcuts?.bindings,
    intentPlanContext,
    visible,
    open,
    warmClaudeCode,
  ]);

  return {
    visible,
    query,
    scope,
    commands,
    matches,
    selectedIndex,
    open,
    close,
    setQuery,
    move,
    activateAt,
    hoverAt,
    editor,
    setEditorDraft,
    submitEditor,
    cancelEditor,
  };
}

function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(target instanceof HTMLElement)) {
    return false;
  }
  if (target.isContentEditable) {
    return true;
  }
  const tagName = target.tagName.toLowerCase();
  return tagName === "input" || tagName === "textarea" || tagName === "select";
}

/**
 * Copy `text` to the clipboard without ever throwing out of the palette's
 * activation path (canonical NSPasteboard cannot fail; here WebView2 /
 * non-secure contexts can reject the async API or omit `navigator.clipboard`
 * entirely, so fall back to the `execCommand` textarea trick and swallow).
 */
async function copyTextToClipboard(text: string): Promise<void> {
  if (navigator.clipboard !== undefined) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall through to the execCommand fallback.
    }
  }
  execCommandCopy(text);
}

/** Legacy-path copy via an off-screen readonly textarea selection. */
function execCommandCopy(text: string): void {
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  try {
    textarea.select();
    document.execCommand("copy");
  } catch (error) {
    // eslint-disable-next-line no-console
    console.error("[command-palette] clipboard copy failed", error);
  } finally {
    textarea.remove();
  }
}

/** Map display commands to the search corpus shape the Rust bridge expects. */
function toCorpus(commands: CommandPaletteCommand[]): CorpusEntry[] {
  return commands.map((command) => ({
    commandId: command.id,
    rank: command.rank,
    title: command.title,
    searchableTexts: searchableTexts({
      title: command.title,
      subtitle: command.subtitle,
      keywords: command.keywords,
    }),
  }));
}
