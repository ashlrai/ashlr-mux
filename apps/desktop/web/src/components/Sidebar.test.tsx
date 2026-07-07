import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";
import { SidebarView } from "./Sidebar";

function ws(overrides: Partial<SessionWorkspaceSnapshot> = {}): SessionWorkspaceSnapshot {
  return { process_title: "Terminal", layout: null, ...overrides };
}

const noop = () => {};

function render(
  workspaces: SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex = 0,
  collapsed = false,
): string {
  return renderToStaticMarkup(
    <SidebarView
      collapsed={collapsed}
      workspaces={workspaces}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={noop}
      onSelectWorkspace={noop}
      onCloseWorkspace={noop}
    />,
  );
}

// Count non-overlapping occurrences of a substring.
function count(haystack: string, needle: string): number {
  return haystack.split(needle).length - 1;
}

describe("SidebarView", () => {
  test("collapsed renders only the hidden rail (no list)", () => {
    const markup = render([ws()], 0, /* collapsed */ true);
    expect(markup).toContain("cmux-sidebar--collapsed");
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain("cmux-sidebar-list");
  });

  test("labels prefer custom_title, then process_title, then Terminal", () => {
    const markup = render([
      ws({ custom_title: "  Custom  " }),
      ws({ process_title: "zsh" }),
      ws({ process_title: "   " }), // blank -> Terminal fallback
    ]);
    expect(markup).toContain(">Custom<");
    expect(markup).toContain(">zsh<");
    expect(markup).toContain(">Terminal<");
  });

  test("marks the selected row and reuses the shared icon", () => {
    const markup = render([ws({ process_title: "a" }), ws({ process_title: "b" })], 1);
    // Exactly one selected row.
    expect(count(markup, "cmux-sidebar-row is-selected")).toBe(1);
    // The reused webviews Icon emits an <svg> per row.
    expect(count(markup, "<svg")).toBe(2);
  });

  test("shows a per-row close button when more than one workspace exists", () => {
    const markup = render([ws({ process_title: "a" }), ws({ process_title: "b" })]);
    expect(count(markup, "cmux-sidebar-row-close")).toBe(2);
  });

  test("hides the close button on the sole workspace (canonical no-op parity)", () => {
    // TabManager.closeWorkspace is a no-op when tabs.count <= 1, so the only
    // remaining workspace exposes no close affordance.
    const markup = render([ws({ process_title: "only" })]);
    expect(markup).not.toContain("cmux-sidebar-row-close");
    // The row and the "new workspace" (+) control still render.
    expect(markup).toContain("cmux-sidebar-row");
    expect(markup).toContain("cmux-sidebar-new");
  });
});
