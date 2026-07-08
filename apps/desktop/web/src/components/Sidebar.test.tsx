import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";
import { SidebarView } from "./Sidebar";

// Deterministic per-call UUIDs so rows are addressable in assertions.
let nextId = 0;
function mintId(): string {
  nextId += 1;
  return `00000000-0000-4000-8000-${String(nextId).padStart(12, "0")}`;
}

function ws(overrides: Partial<SessionWorkspaceSnapshot> = {}): SessionWorkspaceSnapshot {
  return { workspace_id: mintId(), process_title: "Terminal", layout: null, ...overrides };
}

const noop = () => {};

function render(
  workspaces: SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex = 0,
  collapsed = false,
  workspaceGroups?: SessionWorkspaceGroupSnapshot[],
): string {
  return renderToStaticMarkup(
    <SidebarView
      collapsed={collapsed}
      workspaces={workspaces}
      workspaceGroups={workspaceGroups}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={noop}
      onSelectWorkspace={noop}
      onCloseWorkspace={noop}
      onToggleGroupCollapsed={noop}
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

  test("renders a group header for grouped workspaces, suppressing the anchor row", () => {
    const anchor = ws({ process_title: "anchor" });
    const member = ws({ process_title: "member" });
    const gid = mintId();
    anchor.group_id = gid;
    member.group_id = gid;
    const group: SessionWorkspaceGroupSnapshot = {
      id: gid,
      name: "Backend",
      is_collapsed: false,
      anchor_workspace_id: anchor.workspace_id,
    };
    const markup = render([anchor, member, ws({ process_title: "solo" })], 0, false, [group]);

    expect(markup).toContain("cmux-sidebar-group-header");
    expect(markup).toContain(">Backend<");
    // The anchor is represented by the header only; member + solo are rows.
    expect(markup).not.toContain(`data-workspace-id="${anchor.workspace_id}"`);
    expect(markup).toContain(`data-workspace-id="${member.workspace_id}"`);
    // The selected anchor (index 0) marks the header selected.
    expect(markup).toContain("cmux-sidebar-group-header is-selected");
    // The expanded header's chevron exposes the collapse action.
    expect(markup).toContain('aria-label="Collapse group"');
  });

  test("skips rows without a workspace_id (projection parity)", () => {
    const idless: SessionWorkspaceSnapshot = { process_title: "ghost", layout: null };
    const real = ws({ process_title: "real" });
    const markup = render([idless, real], 1);
    expect(markup).not.toContain(">ghost<");
    expect(markup).toContain(">real<");
  });
});
