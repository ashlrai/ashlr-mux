import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type {
  SessionTabManagerSnapshot,
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";
import { SidebarView } from "./Sidebar";

// Fixed UUIDs: the projection skips id-less rows by design (canonical restore
// mints ids in the stateful layer), so every fixture row carries one.
const IDS = [
  "aaaaaaaa-0000-0000-0000-000000000001",
  "aaaaaaaa-0000-0000-0000-000000000002",
  "aaaaaaaa-0000-0000-0000-000000000003",
] as const;
const GID = "bbbbbbbb-0000-0000-0000-000000000001";

function ws(
  index: number,
  overrides: Partial<SessionWorkspaceSnapshot> = {},
): SessionWorkspaceSnapshot {
  return {
    workspace_id: IDS[index],
    process_title: "Terminal",
    layout: null,
    ...overrides,
  };
}

const noop = () => {};

function render(
  workspaces: SessionWorkspaceSnapshot[],
  selectedWorkspaceIndex = 0,
  collapsed = false,
  workspaceGroups?: SessionWorkspaceGroupSnapshot[],
): string {
  const tabs: SessionTabManagerSnapshot = {
    selected_workspace_index: selectedWorkspaceIndex,
    workspaces,
    workspace_groups: workspaceGroups,
  };
  return renderToStaticMarkup(
    <SidebarView
      collapsed={collapsed}
      tabs={tabs}
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
    const markup = render([ws(0)], 0, /* collapsed */ true);
    expect(markup).toContain("cmux-sidebar--collapsed");
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain("cmux-sidebar-list");
  });

  test("null tabs renders an empty (but present) sidebar shell", () => {
    const markup = renderToStaticMarkup(
      <SidebarView
        collapsed={false}
        tabs={null}
        onNewWorkspace={noop}
        onSelectWorkspace={noop}
        onCloseWorkspace={noop}
      />,
    );
    expect(markup).toContain("cmux-sidebar-new");
    expect(markup).not.toContain("cmux-sidebar-row-label");
  });

  test("labels prefer custom_title, then process_title, then Terminal", () => {
    const markup = render([
      ws(0, { custom_title: "  Custom  " }),
      ws(1, { process_title: "zsh" }),
      ws(2, { process_title: "   " }), // blank -> Terminal fallback
    ]);
    expect(markup).toContain(">Custom<");
    expect(markup).toContain(">zsh<");
    expect(markup).toContain(">Terminal<");
  });

  test("marks the selected row and reuses the shared icon", () => {
    const markup = render(
      [ws(0, { process_title: "a" }), ws(1, { process_title: "b" })],
      1,
    );
    // Exactly one selected row.
    expect(count(markup, "is-selected")).toBe(1);
    expect(markup).toMatch(
      new RegExp(`is-selected"[^>]*data-workspace-id="${IDS[1]}"`),
    );
    // The reused webviews Icon emits an <svg> per row.
    expect(count(markup, "<svg")).toBe(2);
  });

  test("shows a per-row close button when more than one workspace exists", () => {
    const markup = render([
      ws(0, { process_title: "a" }),
      ws(1, { process_title: "b" }),
    ]);
    expect(count(markup, "cmux-sidebar-row-close")).toBe(2);
  });

  test("hides the close button on the sole workspace (canonical no-op parity)", () => {
    // TabManager.closeWorkspace is a no-op when tabs.count <= 1, so the only
    // remaining workspace exposes no close affordance.
    const markup = render([ws(0, { process_title: "only" })]);
    expect(markup).not.toContain("cmux-sidebar-row-close");
    // The row and the "new workspace" (+) control still render.
    expect(markup).toContain("cmux-sidebar-row");
    expect(markup).toContain("cmux-sidebar-new");
  });

  test("renders a group header when the snapshot carries workspace groups", () => {
    const markup = render(
      [ws(0, { group_id: GID }), ws(1, { group_id: GID }), ws(2)],
      0,
      false,
      [
        {
          id: GID,
          name: "Project",
          is_collapsed: false,
          anchor_workspace_id: IDS[0],
        },
      ],
    );
    expect(markup).toContain("cmux-sidebar-group-header");
    expect(markup).toContain(">Project<");
    // Anchor suppressed as a plain row; member + solo remain.
    expect(markup).not.toContain(`data-workspace-id="${IDS[0]}"`);
    expect(markup).toContain(`data-workspace-id="${IDS[1]}"`);
    expect(markup).toContain(`data-workspace-id="${IDS[2]}"`);
  });
});
