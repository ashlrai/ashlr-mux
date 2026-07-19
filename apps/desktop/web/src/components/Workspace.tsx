import { useCallback, useEffect, useRef, useState } from "react";

import { createDiffSession } from "../host/diffViewer";
import { host } from "../host/host";
import { recordWaitingInputNotification } from "../host/notifications";
import { usePaneGeometryReporting } from "../hooks/usePaneGeometryReporting";
import { useSession } from "../hooks/useSession";
import {
  agentAttentionPanelForEvent,
  agentAttentionNotificationRequest,
  shouldNotifyAgentPanel,
} from "../session/agentAttention";
import {
  DEFAULT_CANVAS_SNAP_METRICS,
  snapCanvasFrameForMove,
  snapCanvasFrameForResize,
  type CanvasGuide,
  type CanvasResizeEdges,
  type CanvasFrame,
} from "../session/canvasSnap";
import {
  dividerHandles,
  paneRects,
  surfaceKinds,
  type DividerHandle,
  type Rect,
} from "../session/paneRects";
import {
  keyboardDividerResize,
  resizeDivider,
  setDividerAtPath,
  type Layout,
} from "../session/splitLayout";
import { focusedPaneStore, useFocusedPanelId } from "../session/focusedPane";
import { stickyAgentPanes } from "../session/agentMount";
import type { SurfaceKind, SwitchableSurfaceKind } from "../session/surfaceUrl";
import { AgentSessionSurface } from "./AgentSessionSurface";
import { BrowserSurface } from "./BrowserSurface";
import { CustomSidebarSurface } from "./CustomSidebarSurface";
import { DiffSurface } from "./DiffSurface";
import { FileSurface } from "./FileSurface";
import { MarkdownSurface } from "./MarkdownSurface";
import { TerminalSurface } from "./TerminalSurface";
import type {
  PaneBrowserState,
  PaneDiffSession,
  PaneTerminalStartup,
} from "./workspaceSurfaceState";
import type {
  CanvasConfig,
  MarkdownConfig,
  SessionCanvasPaneSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

const PANEL_FLASH_EVENT = "cmux:panel-flash";
const NATIVE_PANEL_FLASH_EVENT = "cmux://panel-flash";
const NATIVE_SURFACE_REFRESH_EVENT = "cmux://refresh-surfaces";

interface PanelFlashDetail {
  panelId: string;
}

export function dispatchPanelFlash(panelId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<PanelFlashDetail>(PANEL_FLASH_EVENT, {
      detail: { panelId },
    }),
  );
}

export function dispatchNativePanelFlash(payload: unknown): void {
  if (typeof payload !== "object" || payload === null || !("panelId" in payload)) {
    return;
  }
  const panelId = (payload as { panelId?: unknown }).panelId;
  if (typeof panelId !== "string" || panelId.trim() === "") {
    return;
  }
  dispatchPanelFlash(panelId);
}

export function dispatchNativeSurfaceRefresh(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event("resize"));
  }
}

export function dispatchPanelFlashSequence(
  panelId: string,
  count = 2,
  intervalMs = 180,
): void {
  if (typeof window === "undefined" || count <= 0) {
    return;
  }
  dispatchPanelFlash(panelId);
  for (let index = 1; index < count; index += 1) {
    window.setTimeout(() => dispatchPanelFlash(panelId), intervalMs * index);
  }
}

/** Thickness of the draggable divider handle, in px (matches `SplitTree`). */
const DIVIDER_PX = 6;
/** Percentage tolerance for "this edge sits on the container boundary". */
const EDGE_EPS = 0.001;
const CANVAS_MIN_PANE_WIDTH = DEFAULT_CANVAS_SNAP_METRICS.minPaneSize.width;
const CANVAS_MIN_PANE_HEIGHT = DEFAULT_CANVAS_SNAP_METRICS.minPaneSize.height;
const CANVAS_MIN_ZOOM = 0.35;
const CANVAS_MAX_ZOOM = 2.5;

interface CanvasViewport {
  x: number;
  y: number;
  scale: number;
}

const CANVAS_RESIZE_HANDLES: ReadonlyArray<{
  key: string;
  label: string;
  edges: CanvasResizeEdges;
}> = [
  { key: "n", label: "from top edge", edges: { top: true } },
  { key: "e", label: "from right edge", edges: { right: true } },
  { key: "s", label: "from bottom edge", edges: { bottom: true } },
  { key: "w", label: "from left edge", edges: { left: true } },
  { key: "ne", label: "from top right corner", edges: { top: true, right: true } },
  { key: "se", label: "from bottom right corner", edges: { bottom: true, right: true } },
  { key: "sw", label: "from bottom left corner", edges: { bottom: true, left: true } },
  { key: "nw", label: "from top left corner", edges: { top: true, left: true } },
];

/**
 * The live, snapshot-driven workspace — the Phase 2 slice 3 replacement for the
 * mock `SplitDemo`.
 *
 * DESIGN: a FLAT PORTAL LAYER. Every terminal is rendered exactly once in an
 * absolutely-positioned layer keyed by its stable `panel_id`, so a split or
 * close (which reshapes the layout tree) never changes a surviving pane's React
 * identity — its `<TerminalSurface>` is not remounted, and its ConPTY shell
 * lives on. Geometry comes from the pure `paneRects` / `dividerHandles`
 * (unit-tested); this component is the DOM shell + pointer wiring + the thin
 * per-pane split/close controls, all driven by `useSession`.
 */
export interface WorkspaceProps {
  canvasConfig?: CanvasConfig | null;
  fileEditorWordWrap?: boolean;
  markdownConfig?: MarkdownConfig | null;
  openTerminalLinksInCmuxBrowser?: boolean;
  showBrowserImportHintOnBlankTabs?: boolean;
  onOpenBrowserImportSettings?: () => void;
  onDismissBrowserImportHint?: () => void;
}

export function Workspace({
  canvasConfig,
  fileEditorWordWrap = false,
  markdownConfig,
  openTerminalLinksInCmuxBrowser = true,
  showBrowserImportHintOnBlankTabs = false,
  onOpenBrowserImportSettings,
  onDismissBrowserImportHint,
}: WorkspaceProps): React.JSX.Element {
  const {
    activeLayout,
    workspaces,
    split,
    close,
    setDivider,
    selectedWorkspaceIndex,
    setSurfaceKind,
    openMarkdownFile,
    openDiffViewer,
    openBrowserUrl,
    browserBack,
    browserForward,
    clearBrowserHistory,
    toggleBrowserOmnibar,
    toggleBrowserFocusMode,
    toggleBrowserDeveloperTools,
    showBrowserDeveloperTools,
    setBrowserZoom,
    setPanelUnread,
    setCanvasPaneFrame,
    applyCanvasAction,
    focusPanel,
  } = useSession();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [flashTokens, setFlashTokens] = useState<Record<string, number>>({});
  const [canvasFrameOverrides, setCanvasFrameOverrides] = useState<
    Map<string, SessionCanvasPaneSnapshot>
  >(new Map());
  const [canvasGuides, setCanvasGuides] = useState<CanvasGuide[]>([]);
  const [canvasViewport, setCanvasViewport] = useState<CanvasViewport>({
    x: 0,
    y: 0,
    scale: 1,
  });

  // Optimistic layout while a divider is dragged: mutate locally for a smooth,
  // round-trip-free drag, then persist once on release. `draggingRef` gates the
  // effect that clears the override so an inbound snapshot mid-drag can't fight
  // the pointer.
  const [override, setOverride] = useState<Layout | null>(null);
  const draggingRef = useRef(false);
  // The single pane that currently owns the one mounted agent surface. Sticky on
  // purpose — see the agent-mount note below the early return for why there is
  // exactly one and why it must survive a toggle back to the terminal.
  const agentPanesRef = useRef<ReadonlySet<string>>(new Set());
  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    const onPanelFlash = (event: Event): void => {
      const detail = (event as CustomEvent<PanelFlashDetail>).detail;
      if (detail == null || typeof detail.panelId !== "string") {
        return;
      }
      setFlashTokens((current) => ({
        ...current,
        [detail.panelId]: (current[detail.panelId] ?? 0) + 1,
      }));
    };
    window.addEventListener(PANEL_FLASH_EVENT, onPanelFlash);
    return () => window.removeEventListener(PANEL_FLASH_EVENT, onPanelFlash);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void host
      .on<PanelFlashDetail>(NATIVE_PANEL_FLASH_EVENT, dispatchNativePanelFlash)
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((error) => {
        console.error("panel flash listener failed", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void host
      .on(NATIVE_SURFACE_REFRESH_EVENT, dispatchNativeSurfaceRefresh)
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((error) => {
        console.error("surface refresh listener failed", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!draggingRef.current) {
      setOverride(null);
    }
  }, [activeLayout]);

  const clearBrowserHistoryWithFlash = useCallback(
    (panelId: string) => {
      dispatchPanelFlashSequence(panelId);
      clearBrowserHistory(panelId);
    },
    [clearBrowserHistory],
  );

  const layout = override ?? activeLayout;

  const beginDividerDrag = useCallback(
    (handle: DividerHandle, base: Layout) =>
      (event: React.PointerEvent<HTMLDivElement>): void => {
        const container = containerRef.current;
        if (!container) {
          return;
        }
        event.preventDefault();
        const horizontal = handle.orientation === "horizontal";
        const rect = container.getBoundingClientRect();
        // The split's axis length in px = its share of the container along the
        // split axis (parent rect is in percentages).
        const axisPixels = horizontal
          ? (rect.width * handle.parent.w) / 100
          : (rect.height * handle.parent.h) / 100;
        const start = horizontal ? event.clientX : event.clientY;
        const startRatio = handle.ratio;
        let lastRatio = startRatio;

        draggingRef.current = true;
        document.body.style.userSelect = "none";
        document.body.style.cursor = horizontal ? "col-resize" : "row-resize";

        // Listen on `window`, not the handle: every move re-renders (state
        // update), which would drop listeners bound to the handle node. An
        // absolute delta from the initial pointer is immune to that churn.
        const onMove = (move: PointerEvent): void => {
          const now = horizontal ? move.clientX : move.clientY;
          lastRatio = resizeDivider(startRatio, now - start, axisPixels);
          setOverride(setDividerAtPath(base, handle.path, lastRatio));
        };
        const onUp = (): void => {
          window.removeEventListener("pointermove", onMove);
          window.removeEventListener("pointerup", onUp);
          document.body.style.userSelect = "";
          document.body.style.cursor = "";
          draggingRef.current = false;
          // Persist the final ratio; the resulting snapshot clears `override`.
          setDivider(handle.path, lastRatio);
        };
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
      },
    [setDivider],
  );

  const handleDividerKeyDown = useCallback(
    (handle: DividerHandle, base: Layout) =>
      (event: React.KeyboardEvent<HTMLDivElement>): void => {
        if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) {
          return;
        }
        const container = containerRef.current;
        if (!container) {
          return;
        }
        const rect = container.getBoundingClientRect();
        const horizontal = handle.orientation === "horizontal";
        const axisPixels = horizontal
          ? (rect.width * handle.parent.w) / 100
          : (rect.height * handle.parent.h) / 100;
        const nextRatio = keyboardDividerResize(
          handle.orientation,
          handle.ratio,
          event.key,
          axisPixels,
        );
        if (nextRatio === null) {
          return;
        }
        event.preventDefault();
        setOverride(setDividerAtPath(base, handle.path, nextRatio));
        setDivider(handle.path, nextRatio);
      },
    [setDivider],
  );

  const currentWorkspace = workspaces[selectedWorkspaceIndex];
  const hasLayout = Boolean(layout);
  const isCurrentWorkspaceCanvas = currentWorkspace?.layout_mode === "canvas";
  const activePanelId = useFocusedPanelId(layout);
  const canvasSnapMetrics = canvasMetricsFromConfig(canvasConfig);
  const canvasSnappingEnabled = canvasConfig?.snappingEnabled ?? true;
  usePaneGeometryReporting(
    containerRef,
    currentWorkspace?.workspace_id,
    hasLayout,
  );

  useEffect(() => {
    if (currentWorkspace?.focused_panel_id !== undefined) {
      focusedPaneStore.focus(currentWorkspace.focused_panel_id);
    }
  }, [currentWorkspace?.focused_panel_id]);

  const recordFocusedPanel = useCallback(
    (panelId: string): void => {
      focusedPaneStore.focus(panelId);
      focusPanel(panelId);
    },
    [focusPanel],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void host
      .on("cmux://agent-event", (event) => {
        if (disposed) {
          return;
        }
        const panelId = agentAttentionPanelForEvent(event, workspaces);
        if (panelId === null) {
          return;
        }
        const documentHasFocus =
          typeof document !== "undefined" && document.hasFocus();
        if (!shouldNotifyAgentPanel(panelId, activePanelId, documentHasFocus)) {
          return;
        }
        dispatchPanelFlash(panelId);
        setPanelUnread(panelId, true);
        const notificationRequest = agentAttentionNotificationRequest(
          panelId,
          workspaces,
        );
        if (notificationRequest !== null) {
          void recordWaitingInputNotification(notificationRequest).catch((error) => {
            console.warn("notification_record_waiting_input failed", error);
          });
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      })
      .catch((error) => {
        if (!disposed) {
          console.warn("agent attention subscription failed", error);
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [activePanelId, setPanelUnread, workspaces]);

  useEffect(() => {
    setCanvasFrameOverrides(new Map());
    setCanvasGuides([]);
  }, [currentWorkspace?.canvas_panes]);

  useEffect(() => {
    setCanvasViewport({ x: 0, y: 0, scale: 1 });
  }, [currentWorkspace?.workspace_id]);

  const handleCanvasWheel = useCallback(
    (event: React.WheelEvent<HTMLDivElement>): void => {
      if (!isCurrentWorkspaceCanvas) {
        return;
      }
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const container = containerRef.current;
        if (!container) {
          return;
        }
        const rect = container.getBoundingClientRect();
        const cursorX = event.clientX - rect.left;
        const cursorY = event.clientY - rect.top;
        setCanvasViewport((current) => {
          const nextScale = Math.min(
            CANVAS_MAX_ZOOM,
            Math.max(
              CANVAS_MIN_ZOOM,
              current.scale * (event.deltaY < 0 ? 1.08 : 0.92),
            ),
          );
          const worldX = (cursorX - current.x) / current.scale;
          const worldY = (cursorY - current.y) / current.scale;
          return {
            x: Math.round(cursorX - worldX * nextScale),
            y: Math.round(cursorY - worldY * nextScale),
            scale: nextScale,
          };
        });
        return;
      }
      setCanvasViewport((current) => ({
        ...current,
        x: Math.round(current.x - event.deltaX),
        y: Math.round(current.y - event.deltaY),
      }));
    },
    [isCurrentWorkspaceCanvas],
  );

  const beginCanvasPaneDrag = useCallback(
    (panelId: string, pane: SessionCanvasPaneSnapshot) =>
      (event: React.PointerEvent<HTMLDivElement>): void => {
        const container = containerRef.current;
        if (!container) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        const scale = 1 / canvasViewport.scale;
        const startX = event.clientX;
        const startY = event.clientY;
        const startFrame = { ...pane };
        const neighbors = canvasPaneNeighbors(
          currentWorkspace?.canvas_panes,
          canvasFrameOverrides,
          panelId,
        );
        let latest = startFrame;

        const onMove = (move: PointerEvent): void => {
          const proposed = {
            ...startFrame,
            x: Math.round(startFrame.x + (move.clientX - startX) * scale),
            y: Math.round(startFrame.y + (move.clientY - startY) * scale),
          };
          if (canvasSnappingEnabled) {
            const snap = snapCanvasFrameForMove(
              proposed,
              neighbors,
              canvasSnapMetrics,
            );
            latest = snap.frame;
            setCanvasGuides(snap.guides);
          } else {
            latest = proposed;
            setCanvasGuides([]);
          }
          setCanvasFrameOverrides((current) => {
            const next = new Map(current);
            next.set(panelId, latest);
            return next;
          });
        };
        const onUp = (): void => {
          window.removeEventListener("pointermove", onMove);
          window.removeEventListener("pointerup", onUp);
          setCanvasGuides([]);
          setCanvasPaneFrame(panelId, latest);
        };
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
    },
    [
      canvasFrameOverrides,
      canvasSnapMetrics,
      canvasSnappingEnabled,
      canvasViewport.scale,
      currentWorkspace?.canvas_panes,
      setCanvasPaneFrame,
    ],
  );

  const beginCanvasPaneResize = useCallback(
    (
      panelId: string,
      pane: SessionCanvasPaneSnapshot,
      edges: CanvasResizeEdges,
    ) =>
      (event: React.PointerEvent<HTMLDivElement>): void => {
        const container = containerRef.current;
        if (!container) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        const scale = 1 / canvasViewport.scale;
        const startX = event.clientX;
        const startY = event.clientY;
        const startFrame = { ...pane };
        const neighbors = canvasPaneNeighbors(
          currentWorkspace?.canvas_panes,
          canvasFrameOverrides,
          panelId,
        );
        let latest = startFrame;

        const onMove = (move: PointerEvent): void => {
          const proposed = resizedCanvasPaneFrame(
            startFrame,
            (move.clientX - startX) * scale,
            (move.clientY - startY) * scale,
            edges,
          );
          if (canvasSnappingEnabled) {
            const snap = snapCanvasFrameForResize(
              proposed,
              edges,
              neighbors,
              canvasSnapMetrics,
            );
            latest = snap.frame;
            setCanvasGuides(snap.guides);
          } else {
            latest = proposed;
            setCanvasGuides([]);
          }
          setCanvasFrameOverrides((current) => {
            const next = new Map(current);
            next.set(panelId, latest);
            return next;
          });
        };
        const onUp = (): void => {
          window.removeEventListener("pointermove", onMove);
          window.removeEventListener("pointerup", onUp);
          setCanvasGuides([]);
          setCanvasPaneFrame(panelId, latest);
        };
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
    },
    [
      canvasFrameOverrides,
      canvasSnapMetrics,
      canvasSnappingEnabled,
      canvasViewport.scale,
      currentWorkspace?.canvas_panes,
      setCanvasPaneFrame,
    ],
  );

  const fitCanvasToView = useCallback((): void => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const rect = container.getBoundingClientRect();
    setCanvasViewport(
      fitCanvasViewport(currentWorkspace?.canvas_panes, rect.width, rect.height),
    );
  }, [currentWorkspace?.canvas_panes]);

  const zoomCanvasBy = useCallback((factor: number): void => {
    const container = containerRef.current;
    const rect = container?.getBoundingClientRect();
    const centerX = rect ? rect.width / 2 : 0;
    const centerY = rect ? rect.height / 2 : 0;
    setCanvasViewport((current) =>
      zoomCanvasViewport(current, factor, centerX, centerY),
    );
  }, []);

  const revealFocusedCanvasPane = useCallback((): void => {
    const container = containerRef.current;
    if (!container || activePanelId === undefined) {
      return;
    }
    const pane = canvasPaneForPanelId(
      currentWorkspace?.canvas_panes,
      canvasFrameOverrides,
      activePanelId,
    );
    if (pane === undefined) {
      return;
    }
    const rect = container.getBoundingClientRect();
    setCanvasViewport((current) =>
      revealCanvasPaneViewport(pane, current, rect.width, rect.height),
    );
  }, [activePanelId, canvasFrameOverrides, currentWorkspace?.canvas_panes]);

  if (!layout) {
    return (
      <div className="flex h-full w-full items-center justify-center text-[12px] text-neutral-500 select-none">
        No panes — the workspace is empty.
      </div>
    );
  }

  const isCanvasLayout = isCurrentWorkspaceCanvas;
  const splitRects = paneRects(layout);
  const baseRects = isCanvasLayout
    ? canvasPaneRects(currentWorkspace, splitRects, canvasFrameOverrides)
    : splitRects;
  const zoomedPanelId = currentWorkspace?.zoomed_panel_id;
  const isSplitZoomed =
    zoomedPanelId !== undefined && baseRects.has(zoomedPanelId);
  const rects = isSplitZoomed
    ? new Map([[zoomedPanelId, { x: 0, y: 0, w: 100, h: 100 } satisfies Rect]])
    : baseRects;
  const handles = isSplitZoomed || isCanvasLayout ? [] : dividerHandles(layout);
  const kinds = surfaceKinds(layout);
  const startupByPanelId = workspaceTerminalStartupByPanelId(workspaces);
  const markdownFilePathByPanelId = workspaceMarkdownFilesByPanelId(workspaces);
  const filePathByPanelId = workspaceFilesByPanelId(workspaces);
  const diffSessionByPanelId = workspaceDiffSessionsByPanelId(workspaces);
  const browserStateByPanelId = workspaceBrowserStatesByPanelId(workspaces);
  const unreadPanelIds = workspaceUnreadPanelIds(workspaces);

  // Which panes own a mounted agent surface. Each agent pane mounts its own
  // surface (events are routed per session id, so concurrent agents are safe),
  // and a surface must survive a toggle back to the terminal: transcript replay
  // covers restored pane bindings, but in-flight runs still keep live activity
  // state in the mounted chat instance. `stickyAgentPanes` encodes those rules.
  // Keeping the owners in a ref makes them sticky across renders; assigning
  // during render is safe (it is not React state and never triggers a re-render).
  const liveAgentPanes = new Set(
    [...kinds].filter(([, kind]) => kind === "agent").map(([panelId]) => panelId),
  );
  agentPanesRef.current = stickyAgentPanes(
    agentPanesRef.current,
    liveAgentPanes,
    new Set(rects.keys()),
  );
  const mountedAgentPanes = agentPanesRef.current;

  return (
    <div
      ref={containerRef}
      className={
        isCanvasLayout
          ? "cmux-workspace-portal cmux-workspace-portal--canvas"
          : "cmux-workspace-portal"
      }
      onWheel={handleCanvasWheel}
      style={{ position: "relative", width: "100%", height: "100%", overflow: "hidden" }}
    >
      {isCanvasLayout ? (
        <div className="cmux-canvas-toolbar" aria-label="Canvas controls">
          <span className="cmux-canvas-toolbar-label">Canvas</span>
          <button type="button" onClick={fitCanvasToView}>
            Overview
          </button>
          <button type="button" onClick={revealFocusedCanvasPane}>
            Reveal
          </button>
          <button type="button" onClick={() => zoomCanvasBy(0.9)}>
            -
          </button>
          <span className="cmux-canvas-toolbar-zoom">
            {Math.round(canvasViewport.scale * 100)}%
          </span>
          <button type="button" onClick={() => zoomCanvasBy(1.1)}>
            +
          </button>
          <button
            type="button"
            onClick={() => setCanvasViewport({ x: 0, y: 0, scale: 1 })}
          >
            100%
          </button>
          <button
            type="button"
            onClick={() =>
              applyCanvasAction("tidy", { paneGap: canvasSnapMetrics.gap })
            }
          >
            Tidy
          </button>
        </div>
      ) : null}
      {isCanvasLayout
        ? canvasGuides.map((guide, index) => (
            <div
              key={`${guide.axis}:${guide.position}:${guide.start}:${guide.end}:${index}`}
              aria-hidden="true"
              className={`cmux-canvas-guide cmux-canvas-guide--${guide.axis}`}
              style={canvasGuideStyle(guide, canvasViewport)}
            />
          ))
        : null}
      {[...rects.entries()].map(([panelId, rect]) => {
        const kind: SurfaceKind = kinds.get(panelId) ?? "terminal";
        const markdownFilePath = markdownFilePathByPanelId.get(panelId);
        const filePath = filePathByPanelId.get(panelId);
        const diffSession = diffSessionByPanelId.get(panelId);
        const browserState = browserStateByPanelId.get(panelId);
        const canvasPaneFrame = isCanvasLayout
          ? canvasPaneForPanelId(
              currentWorkspace?.canvas_panes,
              canvasFrameOverrides,
              panelId,
            )
          : undefined;
        const flashToken = flashTokens[panelId];
        const isUnread = unreadPanelIds.has(panelId);
        const openMarkdown = (): void => {
          if (kind === "markdown") {
            setSurfaceKind(panelId, null);
            return;
          }
          if (markdownFilePath !== undefined && markdownFilePath !== "") {
            setSurfaceKind(panelId, "markdown");
            return;
          }
          void host
            .invoke<string | null>("pick_markdown_file")
            .then((filePath) => {
              if (filePath) {
                openMarkdownFile(panelId, filePath);
              }
            })
            .catch((error) => {
              console.error("pick_markdown_file failed", error);
            });
        };
        const openBrowser = (): void => {
          if (kind === "browser") {
            setSurfaceKind(panelId, null);
            return;
          }
          if (browserState?.url !== undefined && browserState.url !== "") {
            setSurfaceKind(panelId, "browser");
            return;
          }
          openBrowserUrl(panelId);
        };
        const openDiff = (): void => {
          if (kind === "diff") {
            setSurfaceKind(panelId, null);
            return;
          }
          if (diffSession !== undefined) {
            openDiffViewer(panelId, diffSession.token, diffSession.requestPath);
            return;
          }
          void createDiffSession()
            .then((session) => {
              openDiffViewer(panelId, session.token, session.requestPath);
            })
            .catch((error) => {
              console.error("diff_create_session failed", error);
            });
        };
        const createDiffPlaceholderSession = (): void => {
          void createDiffSession()
            .then((session) => {
              openDiffViewer(panelId, session.token, session.requestPath);
            })
            .catch((error) => {
              console.error("diff_create_session failed", error);
            });
        };
        return (
          <div
            key={panelId}
            className="cmux-pane"
            // CAPTURE phase: PaneControls stops pointer-down propagation
            // (bubble-only) and xterm's hidden textarea handles focus
            // internally — neither can block capture, so ANY pointer-down or
            // focus landing inside a pane marks it focused. This is the
            // canonical pane tap gesture + AppKit first-responder sync
            // (WorkspaceContentView.swift:215-217 / 231-238).
            onPointerDownCapture={() => recordFocusedPanel(panelId)}
            onFocusCapture={() => recordFocusedPanel(panelId)}
            style={{
              ...(isCanvasLayout &&
              canvasPaneFrame !== undefined &&
              !isSplitZoomed
                ? canvasPaneStyle(canvasPaneFrame, canvasViewport)
                : paneStyle(rect)),
              overflow: "hidden",
              zIndex: 0,
            }}
          >
            {/* Flat portal: the terminal is rendered once per stable panel_id and
                never unmounted (its ConPTY shell lives on); each owning pane's
                agent surface is overlaid the same way. Toggling terminal⇄agent
                flips which one is VISIBLE — neither is torn down, so an in-flight
                agent run survives an accidental switch.

                The markdown/diff surfaces are DIFFERENT: they hold no
                irrecoverable in-flight state (a scheme-served document, not a
                live session), so they mount ON DEMAND — rendered only while the
                pane hosts that kind, and torn down on switch. */}
            <div style={{ position: "absolute", inset: 0, display: kind === "terminal" ? "flex" : "none", overflow: "hidden" }}>
              <TerminalSurface
                panelId={panelId}
                cwd={startupByPanelId.get(panelId)?.cwd}
                initialCommand={startupByPanelId.get(panelId)?.initialCommand}
                initialInput={startupByPanelId.get(panelId)?.initialInput}
                environment={startupByPanelId.get(panelId)?.environment}
                isActive={terminalPaneIsActive(kind, activePanelId, panelId)}
                onOpenLinkInBrowser={
                  openTerminalLinksInCmuxBrowser
                    ? (url) => openBrowserUrl(panelId, url)
                    : undefined
                }
              />
            </div>
            {mountedAgentPanes.has(panelId) ? (
              <div style={{ position: "absolute", inset: 0, display: kind === "agent" ? "flex" : "none", overflow: "hidden" }}>
                <AgentSessionSurface
                  panelId={panelId}
                  {...(currentWorkspace?.workspace_id ? { workspaceId: currentWorkspace.workspace_id } : {})}
                />
              </div>
            ) : null}
            {kind === "markdown" ? (
              <div style={{ position: "absolute", inset: 0, display: "flex", overflow: "hidden" }}>
                <MarkdownSurface
                  panelId={panelId}
                  filePath={markdownFilePath}
                  markdownConfig={markdownConfig ?? undefined}
                />
              </div>
            ) : null}
            {kind === "file" ? (
              <div style={{ position: "absolute", inset: 0, display: "flex", overflow: "hidden" }}>
                <FileSurface
                  filePath={filePath}
                  wordWrap={fileEditorWordWrap}
                />
              </div>
            ) : null}
            {kind === "diff" ? (
              <div style={{ position: "absolute", inset: 0, display: "flex", overflow: "hidden" }}>
                <DiffSurface
                  panelId={panelId}
                  token={diffSession?.token ?? null}
                  requestPath={diffSession?.requestPath}
                  onCreateSession={createDiffPlaceholderSession}
                />
              </div>
            ) : null}
            {kind === "custom-sidebar" ? (
              <div style={{ position: "absolute", inset: 0, display: "flex", overflow: "hidden" }}>
                <CustomSidebarSurface sourcePath={filePath} />
              </div>
            ) : null}
            {kind === "browser" ? (
              <div style={{ position: "absolute", inset: 0, display: "flex", overflow: "hidden" }}>
                <BrowserSurface
                  panelId={panelId}
                  url={browserState?.url}
                  proxyUrl={browserState?.proxyUrl}
                  zoom={browserState?.zoom}
                  canGoBack={browserState?.canGoBack}
                  canGoForward={browserState?.canGoForward}
                  omnibarVisible={browserState?.omnibarVisible}
                  focusModeActive={browserState?.focusModeActive}
                  developerToolsVisible={browserState?.developerToolsVisible}
                  developerToolsPanel={browserState?.developerToolsPanel}
                  showImportHint={showBrowserImportHintOnBlankTabs}
                  onBack={() => browserBack(panelId)}
                  onForward={() => browserForward(panelId)}
                  onOpenImportHint={onOpenBrowserImportSettings}
                  onOpenImportHintSettings={onOpenBrowserImportSettings}
                  onDismissImportHint={onDismissBrowserImportHint}
                  onToggleOmnibar={() => toggleBrowserOmnibar(panelId)}
                  onToggleFocusMode={() => toggleBrowserFocusMode(panelId)}
                  onToggleDeveloperTools={() => toggleBrowserDeveloperTools(panelId)}
                  onShowDeveloperToolsPanel={(panel) =>
                    showBrowserDeveloperTools(panelId, panel)
                  }
                  onClearHistory={() => clearBrowserHistoryWithFlash(panelId)}
                  onNavigate={(url) => openBrowserUrl(panelId, url)}
                  onZoomChange={(zoom) => setBrowserZoom(panelId, zoom)}
                />
              </div>
            ) : null}
            {flashToken !== undefined ? (
              <div
                key={`${panelId}:${flashToken}`}
                aria-hidden="true"
                className="cmux-pane-focus-flash"
                onAnimationEnd={() => {
                  setFlashTokens((current) => {
                    if (current[panelId] !== flashToken) {
                      return current;
                    }
                    const next = { ...current };
                    delete next[panelId];
                    return next;
                  });
                }}
              />
            ) : null}
            {isUnread ? (
              <div className="cmux-pane-unread-indicator" aria-hidden="true" />
            ) : null}
            {canvasPaneFrame !== undefined ? (
              <div
                className="cmux-canvas-pane-grip"
                title="Drag canvas pane"
                aria-label="Drag canvas pane"
                onPointerDown={beginCanvasPaneDrag(panelId, canvasPaneFrame)}
              >
                Move
              </div>
            ) : null}
            {canvasPaneFrame !== undefined
              ? CANVAS_RESIZE_HANDLES.map((handle) => (
                  <div
                    key={handle.key}
                    className={`cmux-canvas-pane-resize cmux-canvas-pane-resize--${handle.key}`}
                    title={`Resize canvas pane ${handle.label}`}
                    aria-label={`Resize canvas pane ${handle.label}`}
                    onPointerDown={beginCanvasPaneResize(
                      panelId,
                      canvasPaneFrame,
                      handle.edges,
                    )}
                  />
                ))
              : null}
            <PaneControls
              onSplitHorizontal={() => split(panelId, "horizontal")}
              onSplitVertical={() => split(panelId, "vertical")}
              onClose={() => close(panelId)}
              surfaceKind={kind}
              onSetSurfaceKind={(next) => setSurfaceKind(panelId, next)}
              onOpenMarkdown={openMarkdown}
              onOpenBrowser={openBrowser}
              onOpenDiff={openDiff}
              closable={rects.size > 1}
            />
          </div>
        );
      })}
      {handles.map((handle) => (
        <div
          key={handle.path.join("/") || "root"}
          role="separator"
          aria-orientation={handle.orientation === "horizontal" ? "vertical" : "horizontal"}
          aria-valuenow={Math.round(handle.ratio * 100)}
          aria-valuemin={10}
          aria-valuemax={90}
          tabIndex={0}
          onPointerDown={beginDividerDrag(handle, layout)}
          onKeyDown={handleDividerKeyDown(handle, layout)}
          style={dividerStyle(handle)}
        />
      ))}
    </div>
  );
}

function canvasPaneKey(pane: SessionCanvasPaneSnapshot): string {
  return pane.selected_panel_id ?? pane.panel_id;
}

function canvasPaneBounds(
  panes: readonly SessionCanvasPaneSnapshot[] | undefined,
): { minX: number; minY: number; width: number; height: number } {
  const valid = panes?.filter((pane) => pane.width > 0 && pane.height > 0) ?? [];
  if (valid.length === 0) {
    return { minX: 0, minY: 0, width: 1200, height: 800 };
  }
  const minX = Math.min(...valid.map((pane) => pane.x), 0);
  const minY = Math.min(...valid.map((pane) => pane.y), 0);
  const maxX = Math.max(...valid.map((pane) => pane.x + pane.width), 1);
  const maxY = Math.max(...valid.map((pane) => pane.y + pane.height), 1);
  return {
    minX,
    minY,
    width: Math.max(1, maxX - minX),
    height: Math.max(1, maxY - minY),
  };
}

function canvasPaneForPanelId(
  panes: readonly SessionCanvasPaneSnapshot[] | undefined,
  overrides: ReadonlyMap<string, SessionCanvasPaneSnapshot>,
  panelId: string,
): SessionCanvasPaneSnapshot | undefined {
  return (
    overrides.get(panelId) ??
    panes?.find((pane) => canvasPaneKey(pane) === panelId)
  );
}

function fitCanvasViewport(
  panes: readonly SessionCanvasPaneSnapshot[] | undefined,
  viewportWidth: number,
  viewportHeight: number,
): CanvasViewport {
  const bounds = canvasPaneBounds(panes);
  const paddedWidth = Math.max(1, viewportWidth - 64);
  const paddedHeight = Math.max(1, viewportHeight - 64);
  const scale = Math.min(
    CANVAS_MAX_ZOOM,
    Math.max(
      CANVAS_MIN_ZOOM,
      Math.min(paddedWidth / bounds.width, paddedHeight / bounds.height),
    ),
  );
  return {
    x: Math.round((viewportWidth - bounds.width * scale) / 2 - bounds.minX * scale),
    y: Math.round((viewportHeight - bounds.height * scale) / 2 - bounds.minY * scale),
    scale,
  };
}

function zoomCanvasViewport(
  viewport: CanvasViewport,
  factor: number,
  centerX: number,
  centerY: number,
): CanvasViewport {
  const nextScale = Math.min(
    CANVAS_MAX_ZOOM,
    Math.max(CANVAS_MIN_ZOOM, viewport.scale * factor),
  );
  const worldX = (centerX - viewport.x) / viewport.scale;
  const worldY = (centerY - viewport.y) / viewport.scale;
  return {
    x: Math.round(centerX - worldX * nextScale),
    y: Math.round(centerY - worldY * nextScale),
    scale: nextScale,
  };
}

function revealCanvasPaneViewport(
  pane: SessionCanvasPaneSnapshot,
  viewport: CanvasViewport,
  viewportWidth: number,
  viewportHeight: number,
): CanvasViewport {
  const margin = 32;
  const left = pane.x * viewport.scale + viewport.x;
  const right = (pane.x + pane.width) * viewport.scale + viewport.x;
  const top = pane.y * viewport.scale + viewport.y;
  const bottom = (pane.y + pane.height) * viewport.scale + viewport.y;
  let x = viewport.x;
  let y = viewport.y;

  if (left < margin) {
    x += margin - left;
  } else if (right > viewportWidth - margin) {
    x -= right - (viewportWidth - margin);
  }
  if (top < margin) {
    y += margin - top;
  } else if (bottom > viewportHeight - margin) {
    y -= bottom - (viewportHeight - margin);
  }

  return {
    x: Math.round(x),
    y: Math.round(y),
    scale: viewport.scale,
  };
}

function resizedCanvasPaneFrame(
  frame: SessionCanvasPaneSnapshot,
  deltaX: number,
  deltaY: number,
  edges: CanvasResizeEdges,
): SessionCanvasPaneSnapshot {
  let x = frame.x;
  let y = frame.y;
  let width = frame.width;
  let height = frame.height;

  if (edges.right) {
    width = Math.max(CANVAS_MIN_PANE_WIDTH, frame.width + deltaX);
  }
  if (edges.bottom) {
    height = Math.max(CANVAS_MIN_PANE_HEIGHT, frame.height + deltaY);
  }
  if (edges.left) {
    width = Math.max(CANVAS_MIN_PANE_WIDTH, frame.width - deltaX);
    x = frame.x + (frame.width - width);
  }
  if (edges.top) {
    height = Math.max(CANVAS_MIN_PANE_HEIGHT, frame.height - deltaY);
    y = frame.y + (frame.height - height);
  }

  return {
    ...frame,
    x: Math.round(x),
    y: Math.round(y),
    width: Math.round(width),
    height: Math.round(height),
  };
}

function canvasMetricsFromConfig(
  config: CanvasConfig | null | undefined,
): typeof DEFAULT_CANVAS_SNAP_METRICS {
  const paneGap = config?.paneGap;
  const gap =
    typeof paneGap === "number" && Number.isFinite(paneGap) && paneGap >= 0
      ? paneGap
      : DEFAULT_CANVAS_SNAP_METRICS.gap;
  return {
    ...DEFAULT_CANVAS_SNAP_METRICS,
    gap,
  };
}

function canvasPaneRects(
  workspace: SessionWorkspaceSnapshot | undefined,
  fallbackRects: ReadonlyMap<string, Rect>,
  overrides: ReadonlyMap<string, SessionCanvasPaneSnapshot>,
): Map<string, Rect> {
  const panes = (workspace?.canvas_panes ?? [])
    .map((pane) => overrides.get(canvasPaneKey(pane)) ?? pane)
    .filter((pane) => pane.width > 0 && pane.height > 0);
  if (panes.length === 0) {
    return new Map(fallbackRects);
  }

  const { minX, minY, width, height } = canvasPaneBounds(panes);
  const rects = new Map<string, Rect>();

  for (const pane of panes) {
    rects.set(canvasPaneKey(pane), {
      x: ((pane.x - minX) / width) * 100,
      y: ((pane.y - minY) / height) * 100,
      w: (pane.width / width) * 100,
      h: (pane.height / height) * 100,
    });
  }

  // Split mutations can add panes before a future canvas engine persists new
  // frames. Keep those panes visible by borrowing their split geometry.
  for (const [panelId, rect] of fallbackRects) {
    if (!rects.has(panelId)) {
      rects.set(panelId, rect);
    }
  }
  return rects;
}

function canvasPaneNeighbors(
  panes: readonly SessionCanvasPaneSnapshot[] | undefined,
  overrides: ReadonlyMap<string, SessionCanvasPaneSnapshot>,
  activePanelId: string,
): CanvasFrame[] {
  return (panes ?? [])
    .map((pane) => overrides.get(canvasPaneKey(pane)) ?? pane)
    .filter(
      (pane): pane is SessionCanvasPaneSnapshot =>
        canvasPaneKey(pane) !== activePanelId &&
        pane.width > 0 &&
        pane.height > 0,
    )
    .map((pane) => ({
      x: pane.x,
      y: pane.y,
      width: pane.width,
      height: pane.height,
    }));
}

function workspaceTerminalStartupByPanelId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, PaneTerminalStartup> {
  const byPanelId = new Map<string, PaneTerminalStartup>();
  const hasStartup = (startup: PaneTerminalStartup): boolean =>
    startup.cwd != null ||
    startup.initialCommand != null ||
    startup.initialInput != null ||
    startup.environment != null;
  const visit = (
    layout: SessionWorkspaceSnapshot["layout"],
    startup: PaneTerminalStartup | null,
  ): void => {
    if (layout == null || startup == null) {
      return;
    }
    if (layout.type === "pane") {
      for (const panelId of layout.pane.panel_ids) {
        byPanelId.set(panelId, startup);
      }
      if (layout.pane.selected_panel_id != null) {
        byPanelId.set(layout.pane.selected_panel_id, startup);
      }
      return;
    }
    visit(layout.split.first, startup);
    visit(layout.split.second, startup);
  };

  for (const workspace of workspaces) {
    const workspaceStartup: PaneTerminalStartup = {
      cwd: workspace.current_directory,
      initialCommand: workspace.initial_terminal_command,
      initialInput: workspace.initial_terminal_input,
      environment: workspace.initial_terminal_environment as Record<string, string> | undefined,
    };
    if (hasStartup(workspaceStartup)) {
      visit(workspace.layout, workspaceStartup);
    }
    for (const override of workspace.panel_terminal_startups ?? []) {
      const startup: PaneTerminalStartup = {
        cwd: workspace.current_directory,
        initialCommand: override.initial_terminal_command,
        initialInput: override.initial_terminal_input,
        environment: override.initial_terminal_environment as Record<string, string> | undefined,
      };
      if (hasStartup(startup)) {
        byPanelId.set(override.panel_id, startup);
      }
    }
  }
  return byPanelId;
}

function workspaceMarkdownFilesByPanelId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, string> {
  const byPanelId = new Map<string, string>();
  const visit = (layout: SessionWorkspaceSnapshot["layout"]): void => {
    if (layout == null) {
      return;
    }
    if (layout.type === "pane") {
      const filePath = layout.pane.markdown_file_path;
      if (filePath != null && filePath !== "") {
        for (const panelId of layout.pane.panel_ids) {
          byPanelId.set(panelId, filePath);
        }
        if (layout.pane.selected_panel_id != null) {
          byPanelId.set(layout.pane.selected_panel_id, filePath);
        }
      }
      return;
    }
    visit(layout.split.first);
    visit(layout.split.second);
  };

  for (const workspace of workspaces) {
    visit(workspace.layout);
  }
  return byPanelId;
}

function workspaceFilesByPanelId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, string> {
  const byPanelId = new Map<string, string>();
  const visit = (layout: SessionWorkspaceSnapshot["layout"]): void => {
    if (layout == null) {
      return;
    }
    if (layout.type === "pane") {
      const filePath = layout.pane.file_path;
      if (filePath != null && filePath !== "") {
        for (const panelId of layout.pane.panel_ids) {
          byPanelId.set(panelId, filePath);
        }
        if (layout.pane.selected_panel_id != null) {
          byPanelId.set(layout.pane.selected_panel_id, filePath);
        }
      }
      return;
    }
    visit(layout.split.first);
    visit(layout.split.second);
  };

  for (const workspace of workspaces) {
    visit(workspace.layout);
  }
  return byPanelId;
}

function workspaceDiffSessionsByPanelId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, PaneDiffSession> {
  const byPanelId = new Map<string, PaneDiffSession>();
  const visit = (layout: SessionWorkspaceSnapshot["layout"]): void => {
    if (layout == null) {
      return;
    }
    if (layout.type === "pane") {
      const token = layout.pane.diff_viewer_token;
      if (token != null && token !== "") {
        const session: PaneDiffSession = {
          token,
          requestPath: layout.pane.diff_viewer_request_path ?? undefined,
        };
        for (const panelId of layout.pane.panel_ids) {
          byPanelId.set(panelId, session);
        }
        if (layout.pane.selected_panel_id != null) {
          byPanelId.set(layout.pane.selected_panel_id, session);
        }
      }
      return;
    }
    visit(layout.split.first);
    visit(layout.split.second);
  };

  for (const workspace of workspaces) {
    visit(workspace.layout);
  }
  return byPanelId;
}

function workspaceBrowserStatesByPanelId(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, PaneBrowserState> {
  const byPanelId = new Map<string, PaneBrowserState>();
  const visit = (layout: SessionWorkspaceSnapshot["layout"]): void => {
    if (layout == null) {
      return;
    }
    if (layout.type === "pane") {
      const availability = browserHistoryNavigationAvailability(
        layout.pane.browser_back_history,
        layout.pane.browser_forward_history,
      );
      const state: PaneBrowserState = {
        url: layout.pane.browser_url ?? undefined,
        proxyUrl: layout.pane.browser_proxy_url ?? undefined,
        zoom: layout.pane.browser_page_zoom ?? undefined,
        canGoBack: availability.canGoBack,
        canGoForward: availability.canGoForward,
        omnibarVisible: layout.pane.browser_omnibar_visible ?? true,
        focusModeActive: layout.pane.browser_focus_mode_active ?? false,
        developerToolsVisible: layout.pane.browser_developer_tools_visible ?? false,
        developerToolsPanel: layout.pane.browser_developer_tools_panel ?? undefined,
      };
      if (
        state.url !== undefined ||
        state.proxyUrl !== undefined ||
        state.zoom !== undefined ||
        state.canGoBack ||
        state.canGoForward ||
        !state.omnibarVisible ||
        state.focusModeActive ||
        state.developerToolsVisible ||
        state.developerToolsPanel !== undefined
      ) {
        for (const panelId of layout.pane.panel_ids) {
          byPanelId.set(panelId, state);
        }
        if (layout.pane.selected_panel_id != null) {
          byPanelId.set(layout.pane.selected_panel_id, state);
        }
      }
      return;
    }
    visit(layout.split.first);
    visit(layout.split.second);
  };

  for (const workspace of workspaces) {
    visit(workspace.layout);
  }
  return byPanelId;
}

function isTemporaryBrowserHistoryUrl(url: URL): boolean {
  if (
    url.protocol === "cmux-diff-viewer:" ||
    url.protocol === "cmux-remote-image:"
  ) {
    return true;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return false;
  }
  return (
    url.hostname.toLowerCase() === "cmux-diff-viewer.localhost" ||
    url.hostname.toLowerCase() === "cmux-remote-image.localhost"
  );
}

export function isSerializableBrowserHistoryUrl(rawUrl: string): boolean {
  const trimmed = rawUrl.trim();
  if (trimmed === "" || trimmed.toLowerCase() === "about:blank") {
    return false;
  }
  try {
    return !isTemporaryBrowserHistoryUrl(new URL(trimmed));
  } catch {
    return false;
  }
}

export function browserHistoryNavigationAvailability(
  backHistory?: readonly string[] | null,
  forwardHistory?: readonly string[] | null,
): { canGoBack: boolean; canGoForward: boolean } {
  return {
    canGoBack: backHistory?.some(isSerializableBrowserHistoryUrl) ?? false,
    canGoForward: forwardHistory?.some(isSerializableBrowserHistoryUrl) ?? false,
  };
}

export function terminalPaneIsActive(
  kind: SurfaceKind,
  activePanelId: string | undefined,
  panelId: string,
): boolean {
  return kind === "terminal" && activePanelId === panelId;
}

function workspaceUnreadPanelIds(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Set<string> {
  const ids = new Set<string>();
  for (const workspace of workspaces) {
    for (const entry of workspace.panel_unreads ?? []) {
      if (entry.is_unread) {
        ids.add(entry.panel_id);
      }
    }
  }
  return ids;
}

/** Absolute position + size for a pane, inset by half a divider on inner edges. */
function paneStyle(rect: Rect): React.CSSProperties {
  const half = DIVIDER_PX / 2;
  const left = rect.x > EDGE_EPS ? half : 0;
  const right = rect.x + rect.w < 100 - EDGE_EPS ? half : 0;
  const top = rect.y > EDGE_EPS ? half : 0;
  const bottom = rect.y + rect.h < 100 - EDGE_EPS ? half : 0;
  return {
    position: "absolute",
    left: `calc(${rect.x}% + ${left}px)`,
    top: `calc(${rect.y}% + ${top}px)`,
    width: `calc(${rect.w}% - ${left + right}px)`,
    height: `calc(${rect.h}% - ${top + bottom}px)`,
    minWidth: 0,
    minHeight: 0,
  };
}

/** Absolute canvas frame after viewport pan/zoom. Uses persisted canvas units. */
function canvasPaneStyle(
  frame: SessionCanvasPaneSnapshot,
  viewport: CanvasViewport,
): React.CSSProperties {
  return {
    position: "absolute",
    left: `${Math.round(frame.x * viewport.scale + viewport.x)}px`,
    top: `${Math.round(frame.y * viewport.scale + viewport.y)}px`,
    width: `${Math.round(frame.width * viewport.scale)}px`,
    height: `${Math.round(frame.height * viewport.scale)}px`,
    minWidth: 0,
    minHeight: 0,
  };
}

function canvasGuideStyle(
  guide: CanvasGuide,
  viewport: CanvasViewport,
): React.CSSProperties {
  if (guide.axis === "vertical") {
    return {
      left: `${Math.round(guide.position * viewport.scale + viewport.x)}px`,
      top: `${Math.round(guide.start * viewport.scale + viewport.y)}px`,
      height: `${Math.max(1, Math.round((guide.end - guide.start) * viewport.scale))}px`,
    };
  }
  return {
    left: `${Math.round(guide.start * viewport.scale + viewport.x)}px`,
    top: `${Math.round(guide.position * viewport.scale + viewport.y)}px`,
    width: `${Math.max(1, Math.round((guide.end - guide.start) * viewport.scale))}px`,
  };
}

/** The 6px divider strip, centered on the split boundary, above the panes. */
function dividerStyle(handle: DividerHandle): React.CSSProperties {
  const half = DIVIDER_PX / 2;
  const { parent, ratio, orientation } = handle;
  const base: React.CSSProperties = {
    position: "absolute",
    zIndex: 10,
    background: "#1c2230",
    touchAction: "none",
  };
  if (orientation === "horizontal") {
    const centerline = parent.x + parent.w * ratio;
    return {
      ...base,
      left: `calc(${centerline}% - ${half}px)`,
      top: `${parent.y}%`,
      width: `${DIVIDER_PX}px`,
      height: `${parent.h}%`,
      cursor: "col-resize",
    };
  }
  const centerline = parent.y + parent.h * ratio;
  return {
    ...base,
    top: `calc(${centerline}% - ${half}px)`,
    left: `${parent.x}%`,
    height: `${DIVIDER_PX}px`,
    width: `${parent.w}%`,
    cursor: "row-resize",
  };
}

interface PaneControlsProps {
  onSplitHorizontal: () => void;
  onSplitVertical: () => void;
  onClose: () => void;
  /** The pane's current surface. Drives which toggle reads as "active". */
  surfaceKind: SurfaceKind;
  /**
   * Set the pane's surface: a surface tag to switch to it, or `null` to revert
   * to the terminal. Each toggle button flips its kind on⇄terminal.
   */
  onSetSurfaceKind: (kind: SwitchableSurfaceKind | null) => void;
  /** Open or reactivate the pane's markdown document. */
  onOpenMarkdown: () => void;
  /** Open or reactivate the pane's browser surface. */
  onOpenBrowser: () => void;
  /** Open or reactivate the pane's diff viewer surface. */
  onOpenDiff: () => void;
  closable: boolean;
}

/**
 * Tiny hover-in-corner controls: toggle each non-terminal surface (agent /
 * markdown / diff), split side-by-side, split stacked, close. Each surface
 * toggle switches the pane to that kind, or back to the terminal when it is
 * already the active surface.
 */
function PaneControls({
  onSplitHorizontal,
  onSplitVertical,
  onClose,
  surfaceKind,
  onSetSurfaceKind,
  onOpenMarkdown,
  onOpenBrowser,
  onOpenDiff,
  closable,
}: PaneControlsProps): React.JSX.Element {
  const toggle = (kind: SwitchableSurfaceKind): void =>
    onSetSurfaceKind(surfaceKind === kind ? null : kind);
  return (
    <div
      className="cmux-pane-controls"
      style={{ position: "absolute", top: 4, right: 4, zIndex: 20, display: "flex", gap: 2 }}
    >
      <ControlButton
        label={surfaceKind === "agent" ? "Switch to terminal" : "Start agent session"}
        active={surfaceKind === "agent"}
        onClick={() => toggle("agent")}
      >
        {surfaceKind === "agent" ? "⌨" : "✦"}
      </ControlButton>
      <ControlButton
        label={surfaceKind === "markdown" ? "Switch to terminal" : "Markdown preview"}
        active={surfaceKind === "markdown"}
        onClick={onOpenMarkdown}
      >
        ✎
      </ControlButton>
      {surfaceKind === "file" ? (
        <ControlButton
          label="Switch to terminal"
          active={true}
          onClick={() => onSetSurfaceKind(null)}
        >
          F
        </ControlButton>
      ) : null}
      {surfaceKind === "custom-sidebar" ? (
        <ControlButton
          label="Switch to terminal"
          active={true}
          onClick={() => onSetSurfaceKind(null)}
        >
          S
        </ControlButton>
      ) : null}
      <ControlButton
        label={surfaceKind === "browser" ? "Switch to terminal" : "Browser"}
        active={surfaceKind === "browser"}
        onClick={onOpenBrowser}
      >
        B
      </ControlButton>
      <ControlButton
        label={surfaceKind === "diff" ? "Switch to terminal" : "Diff viewer"}
        active={surfaceKind === "diff"}
        onClick={onOpenDiff}
      >
        ±
      </ControlButton>
      <ControlButton label="Split side by side" onClick={onSplitHorizontal}>
        ▐
      </ControlButton>
      <ControlButton label="Split stacked" onClick={onSplitVertical}>
        ▄
      </ControlButton>
      {closable ? (
        <ControlButton label="Close pane" onClick={onClose}>
          ✕
        </ControlButton>
      ) : null}
    </div>
  );
}

function ControlButton({
  label,
  onClick,
  active = false,
  children,
}: {
  label: string;
  onClick: () => void;
  /** Whether this button's surface is the pane's active surface. */
  active?: boolean;
  children: React.ReactNode;
}): React.JSX.Element {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      // Pointer-down on a control must not start a terminal focus / divider drag.
      onPointerDown={(event) => event.stopPropagation()}
      onClick={onClick}
      className={
        active
          ? "flex h-5 w-5 items-center justify-center rounded bg-neutral-600 text-[10px] text-neutral-100"
          : "flex h-5 w-5 items-center justify-center rounded bg-neutral-800/80 text-[10px] text-neutral-300 hover:bg-neutral-700 hover:text-neutral-100"
      }
    >
      {children}
    </button>
  );
}
