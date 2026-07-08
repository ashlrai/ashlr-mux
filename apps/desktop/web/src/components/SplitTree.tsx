import { useCallback, useRef } from "react";

import {
  clampDivider,
  keyboardDividerResize,
  resizeDivider,
  type Layout,
  type Pane,
  type SplitPath,
} from "../session/splitLayout";

/** Thickness of the draggable divider handle, in px. */
const DIVIDER_PX = 6;

export interface SplitTreeProps {
  /** The layout subtree to render (a pane leaf or a split node). */
  layout: Layout;
  /** Render a leaf pane. `path` locates it in the tree for keying/focus. */
  renderPane: (pane: Pane, path: SplitPath) => React.ReactNode;
  /** Called with the target split's path + new ratio while a divider is dragged. */
  onDividerChange?: (path: SplitPath, position: number) => void;
}

/**
 * Renders a `@cmux/core-types` session layout as nested flex containers with
 * draggable dividers — the Windows/React equivalent of the macOS bonsplit view.
 * The split math lives in `../session/splitLayout` (pure, unit-tested); this
 * component is just the DOM shell + pointer wiring.
 */
export function SplitTree({ layout, renderPane, onDividerChange }: SplitTreeProps): React.JSX.Element {
  return <SplitNode layout={layout} path={[]} renderPane={renderPane} onDividerChange={onDividerChange} />;
}

interface SplitNodeProps extends SplitTreeProps {
  path: SplitPath;
}

function SplitNode({ layout, path, renderPane, onDividerChange }: SplitNodeProps): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement | null>(null);

  const onDividerPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (layout.type !== "split" || !onDividerChange) {
        return;
      }
      const container = containerRef.current;
      if (!container) {
        return;
      }
      event.preventDefault();
      const horizontal = layout.split.orientation === "horizontal";
      const rect = container.getBoundingClientRect();
      const axisPixels = horizontal ? rect.width : rect.height;
      const start = horizontal ? event.clientX : event.clientY;
      const startPosition = layout.split.divider_position;

      // Listen on `window`, not the handle: the drag triggers a re-render every
      // move (state update), which would drop listeners/pointer-capture bound to
      // the handle node. Window listeners + an absolute delta from the initial
      // pointer position are immune to that churn.
      const onMove = (move: PointerEvent): void => {
        const now = horizontal ? move.clientX : move.clientY;
        onDividerChange(path, resizeDivider(startPosition, now - start, axisPixels));
      };
      const onUp = (): void => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        document.body.style.userSelect = "";
        document.body.style.cursor = "";
      };
      // Keep the resize cursor + suppress text selection for the whole drag.
      document.body.style.userSelect = "none";
      document.body.style.cursor = horizontal ? "col-resize" : "row-resize";
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [layout, onDividerChange, path],
  );

  const onDividerKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (layout.type !== "split" || !onDividerChange) {
        return;
      }
      // Plain arrows only: modified arrows belong to app/browser chords.
      if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) {
        return;
      }
      const container = containerRef.current;
      if (!container) {
        return;
      }
      const rect = container.getBoundingClientRect();
      const axisPixels = layout.split.orientation === "horizontal" ? rect.width : rect.height;
      const position = keyboardDividerResize(
        layout.split.orientation,
        layout.split.divider_position,
        event.key,
        axisPixels,
      );
      if (position === null) {
        // Unhandled keys (incl. cross-axis arrows) pass through untouched.
        return;
      }
      event.preventDefault();
      onDividerChange(path, position);
    },
    [layout, onDividerChange, path],
  );

  if (layout.type === "pane") {
    return <div className="cmux-pane">{renderPane(layout.pane, path)}</div>;
  }

  const { orientation, divider_position, first, second } = layout.split;
  const horizontal = orientation === "horizontal";
  const ratio = clampDivider(divider_position);

  // `overflow: hidden` on every cell is essential: pane content (notably the
  // xterm canvas, which the fit addon sizes in fixed px) must never paint
  // outside its flex cell, or it covers the divider and the sibling panes.
  const cellBase: React.CSSProperties = { overflow: "hidden", minWidth: 0, minHeight: 0 };

  return (
    <div
      ref={containerRef}
      className="cmux-split"
      style={{
        display: "flex",
        flexDirection: horizontal ? "row" : "column",
        width: "100%",
        height: "100%",
        overflow: "hidden",
        minWidth: 0,
        minHeight: 0,
      }}
    >
      <div style={{ ...cellBase, flex: `0 0 calc(${ratio * 100}% - ${DIVIDER_PX / 2}px)` }}>
        <SplitNode
          layout={first}
          path={[...path, "first"]}
          renderPane={renderPane}
          onDividerChange={onDividerChange}
        />
      </div>
      <div
        className="cmux-split-divider"
        role="separator"
        aria-orientation={horizontal ? "vertical" : "horizontal"}
        tabIndex={0}
        aria-valuenow={Math.round(ratio * 100)}
        aria-valuemin={10}
        aria-valuemax={90}
        onPointerDown={onDividerPointerDown}
        onKeyDown={onDividerKeyDown}
        style={{
          position: "relative",
          zIndex: 1,
          flex: `0 0 ${DIVIDER_PX}px`,
          cursor: horizontal ? "col-resize" : "row-resize",
          background: "#1c2230",
          touchAction: "none",
        }}
      />
      <div style={{ ...cellBase, flex: "1 1 0" }}>
        <SplitNode
          layout={second}
          path={[...path, "second"]}
          renderPane={renderPane}
          onDividerChange={onDividerChange}
        />
      </div>
    </div>
  );
}
