import { useCallback, useEffect, useRef, useState } from "react";

import { useSession } from "../hooks/useSession";
import {
  dividerHandles,
  paneRects,
  type DividerHandle,
  type Rect,
} from "../session/paneRects";
import { resizeDivider, setDividerAtPath, type Layout } from "../session/splitLayout";
import { TerminalSurface } from "./TerminalSurface";

/** Thickness of the draggable divider handle, in px (matches `SplitTree`). */
const DIVIDER_PX = 6;
/** Percentage tolerance for "this edge sits on the container boundary". */
const EDGE_EPS = 0.001;

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
export function Workspace(): React.JSX.Element {
  const { activeLayout, split, close, setDivider } = useSession();
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Optimistic layout while a divider is dragged: mutate locally for a smooth,
  // round-trip-free drag, then persist once on release. `draggingRef` gates the
  // effect that clears the override so an inbound snapshot mid-drag can't fight
  // the pointer.
  const [override, setOverride] = useState<Layout | null>(null);
  const draggingRef = useRef(false);
  useEffect(() => {
    if (!draggingRef.current) {
      setOverride(null);
    }
  }, [activeLayout]);

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

  if (!layout) {
    return (
      <div className="flex h-full w-full items-center justify-center text-[12px] text-neutral-500 select-none">
        No panes — the workspace is empty.
      </div>
    );
  }

  const rects = paneRects(layout);
  const handles = dividerHandles(layout);

  return (
    <div ref={containerRef} className="cmux-workspace-portal" style={{ position: "relative", width: "100%", height: "100%", overflow: "hidden" }}>
      {[...rects.entries()].map(([panelId, rect]) => (
        <div key={panelId} style={{ ...paneStyle(rect), display: "flex", overflow: "hidden", zIndex: 0 }}>
          <TerminalSurface />
          <PaneControls
            onSplitHorizontal={() => split(panelId, "horizontal")}
            onSplitVertical={() => split(panelId, "vertical")}
            onClose={() => close(panelId)}
            closable={rects.size > 1}
          />
        </div>
      ))}
      {handles.map((handle) => (
        <div
          key={handle.path.join("/") || "root"}
          role="separator"
          aria-orientation={handle.orientation === "horizontal" ? "vertical" : "horizontal"}
          onPointerDown={beginDividerDrag(handle, layout)}
          style={dividerStyle(handle)}
        />
      ))}
    </div>
  );
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
  closable: boolean;
}

/** Tiny hover-in-corner controls: split side-by-side, split stacked, close. */
function PaneControls({ onSplitHorizontal, onSplitVertical, onClose, closable }: PaneControlsProps): React.JSX.Element {
  return (
    <div
      className="cmux-pane-controls"
      style={{ position: "absolute", top: 4, right: 4, zIndex: 20, display: "flex", gap: 2 }}
    >
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
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}): React.JSX.Element {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      // Pointer-down on a control must not start a terminal focus / divider drag.
      onPointerDown={(event) => event.stopPropagation()}
      onClick={onClick}
      className="flex h-5 w-5 items-center justify-center rounded bg-neutral-800/80 text-[10px] text-neutral-300 hover:bg-neutral-700 hover:text-neutral-100"
    >
      {children}
    </button>
  );
}
