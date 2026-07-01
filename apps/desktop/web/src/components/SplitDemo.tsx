import { useState } from "react";

import type { Layout, Pane, SplitPath } from "../session/splitLayout";
import { setDividerAtPath } from "../session/splitLayout";
import { TerminalSurface } from "./TerminalSurface";
import { SplitTree } from "./SplitTree";

/**
 * A throwaway visual demo of the Phase 2 split renderer with MOCK data: a live
 * terminal beside two stacked mock panes, all dividers draggable. This proves
 * the `SplitTree` renderer + divider math before the real Rust session-state
 * backend exists. Replaced by a live-snapshot-driven workspace in slice 3.
 */
const INITIAL_LAYOUT: Layout = {
  type: "split",
  split: {
    orientation: "horizontal",
    divider_position: 0.55,
    // Left: the real ConPTY terminal.
    first: { type: "pane", pane: { panel_ids: ["terminal"] } },
    // Right: two mock panes stacked vertically.
    second: {
      type: "split",
      split: {
        orientation: "vertical",
        divider_position: 0.5,
        first: { type: "pane", pane: { panel_ids: ["mock:B"] } },
        second: { type: "pane", pane: { panel_ids: ["mock:C"] } },
      },
    },
  },
};

const MOCK_COLORS: Record<string, string> = {
  "mock:B": "#132030",
  "mock:C": "#1c1330",
};

export function SplitDemo(): React.JSX.Element {
  const [layout, setLayout] = useState<Layout>(INITIAL_LAYOUT);

  const renderPane = (pane: Pane, path: SplitPath): React.ReactNode => {
    const id = pane.panel_ids[0];
    if (id === "terminal") {
      return <TerminalSurface />;
    }
    return (
      <div
        className="flex h-full w-full items-center justify-center text-[12px] text-neutral-400 select-none"
        style={{ background: MOCK_COLORS[id] ?? "#101010" }}
      >
        <span>
          {id} <span className="text-neutral-600">· path [{path.join(" → ") || "root"}]</span>
        </span>
      </div>
    );
  };

  return (
    <SplitTree
      layout={layout}
      renderPane={renderPane}
      onDividerChange={(path, position) => setLayout((prev) => setDividerAtPath(prev, path, position))}
    />
  );
}
