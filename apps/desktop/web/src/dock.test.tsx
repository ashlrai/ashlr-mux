import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { DockPanel } from "./components/DockPanel";
import {
  dockCurrentSurface,
  dockReducer,
  type DockSnapshot,
} from "./dock";

const snapshot: DockSnapshot = {
  owner_id: "window-1",
  focused_pane_id: "pane-2",
  panes: [
    {
      id: "pane-1",
      surface_ids: ["terminal-1", "browser-1"],
      selected_surface_id: "browser-1",
      placement: "root",
      divider_position: null,
    },
    {
      id: "pane-2",
      surface_ids: ["terminal-2"],
      selected_surface_id: "terminal-2",
      placement: "split_down",
      divider_position: 0.4,
    },
  ],
  surfaces: [
    {
      id: "terminal-1",
      pane_id: "pane-1",
      kind: "terminal",
      title: "Tests",
      runtime: { type: "terminal", working_directory: "C:/repo", command: "bun test", environment: {} },
    },
    {
      id: "browser-1",
      pane_id: "pane-1",
      kind: "browser",
      title: "Docs",
      runtime: { type: "browser", url: "https://example.com", profile: null },
    },
    {
      id: "terminal-2",
      pane_id: "pane-2",
      kind: "terminal",
      title: "Logs",
      runtime: { type: "terminal", working_directory: null, command: "tail -f app.log", environment: {} },
    },
  ],
};

describe("Dock UI state", () => {
  test("keeps pane selection separate from focused Dock surface", () => {
    expect(dockCurrentSurface(snapshot)?.id).toBe("terminal-2");
    const selected = dockReducer(snapshot, {
      type: "selected",
      paneId: "pane-1",
      surfaceId: "terminal-1",
      focus: false,
    });
    expect(selected.panes[0]?.selected_surface_id).toBe("terminal-1");
    expect(dockCurrentSurface(selected)?.id).toBe("terminal-2");
    const focused = dockReducer(selected, { type: "focused", surfaceId: "terminal-1" });
    expect(dockCurrentSurface(focused)?.id).toBe("terminal-1");
  });

  test("renders ordered heterogeneous panes with actionable tabs", () => {
    const markup = renderToStaticMarkup(<DockPanel snapshot={snapshot} />);
    expect(markup.indexOf("Tests")).toBeLessThan(markup.indexOf("Docs"));
    expect(markup.indexOf("Docs")).toBeLessThan(markup.indexOf("Logs"));
    expect(markup).toContain('data-dock-kind="terminal"');
    expect(markup).toContain('data-dock-kind="browser"');
    expect(markup).toContain('aria-label="Close Docs"');
    expect(markup).toContain('aria-label="New Dock terminal"');
    expect(markup).toContain('aria-label="New Dock browser"');
  });
});
