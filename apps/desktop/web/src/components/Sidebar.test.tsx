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
  multiSelectedWorkspaceIds?: ReadonlySet<string>,
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
      onRenameWorkspace={noop}
      onSetWorkspacePinned={noop}
      onToggleGroupCollapsed={noop}
      multiSelectedWorkspaceIds={multiSelectedWorkspaceIds}
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
    // The reused webviews Icon emits an <svg> per row icon and per pin toggle.
    expect(count(markup, "<svg")).toBe(4);
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

  test("a custom_title labels its row (the rename prefill source)", () => {
    // Ties workspaceTitle (custom_title || process_title || "Terminal") to the
    // rename affordance: the row label IS the inline editor's prefill.
    const markup = render([ws({ custom_title: "Named", process_title: "zsh" })]);
    expect(markup).toContain(">Named<");
    expect(markup).not.toContain(">zsh<");
  });

  test("rows render the plain label span by default (no stray rename input)", () => {
    const markup = render([ws({ process_title: "a" }), ws({ process_title: "b" })]);
    expect(markup).toContain("cmux-sidebar-row-label");
    expect(markup).not.toContain("cmux-sidebar-row-rename");
  });

  test("renders a pin toggle per row with pin-state labels", () => {
    // Persisted order is rendered as-is (the pinned-ahead reorder itself is
    // Rust-tested); each id-bearing row gets the affordance, labelled by its
    // own pin state. The id → index wiring goes through the same `withIndexOf`
    // adapter the close/rename affordances already exercise.
    const markup = render([
      ws({ process_title: "a", is_pinned: true }),
      ws({ process_title: "b" }),
    ]);
    expect(count(markup, "cmux-sidebar-row-pin")).toBe(2);
    expect(markup).toContain('aria-label="Unpin a"');
    expect(markup).toContain('aria-label="Pin b"');
    // The pinned row precedes the unpinned one (persisted order preserved).
    expect(markup.indexOf('aria-label="Unpin a"')).toBeLessThan(
      markup.indexOf('aria-label="Pin b"'),
    );
  });

  test("skips rows without a workspace_id (projection parity)", () => {
    const idless: SessionWorkspaceSnapshot = { process_title: "ghost", layout: null };
    const real = ws({ process_title: "real" });
    const markup = render([idless, real], 1);
    expect(markup).not.toContain(">ghost<");
    expect(markup).toContain(">real<");
  });

  test("multi-selected rows render alongside the single active row", () => {
    const rows = [ws({ process_title: "a" }), ws({ process_title: "b" }), ws({ process_title: "c" })];
    const markup = render(
      rows,
      0,
      false,
      undefined,
      new Set([rows[1]!.workspace_id!]),
    );
    // One active row, one multi-selected row, one plain row.
    expect(count(markup, "cmux-sidebar-row is-selected")).toBe(1);
    expect(count(markup, "cmux-sidebar-row is-multi-selected")).toBe(1);
    expect(count(markup, 'aria-selected="true"')).toBe(2);
  });

  test("a multi-selected id equal to the active id renders is-selected only", () => {
    // Canonical isActive-first precedence (SidebarAppearanceSupport.swift:
    // 312-336): the active row never doubles as multi-selected.
    const rows = [ws({ process_title: "a" }), ws({ process_title: "b" })];
    const markup = render(
      rows,
      0,
      false,
      undefined,
      new Set([rows[0]!.workspace_id!]),
    );
    expect(count(markup, "cmux-sidebar-row is-selected")).toBe(1);
    expect(markup).not.toContain("is-multi-selected");
  });

  test("a group header draws multi-selection via its anchor workspace", () => {
    const anchor = ws({ process_title: "anchor" });
    const member = ws({ process_title: "member" });
    const solo = ws({ process_title: "solo" });
    const gid = mintId();
    anchor.group_id = gid;
    member.group_id = gid;
    const group: SessionWorkspaceGroupSnapshot = {
      id: gid,
      name: "Backend",
      is_collapsed: false,
      anchor_workspace_id: anchor.workspace_id,
    };
    // The solo row (index 2) is active; the group anchor is multi-selected.
    const markup = render(
      [anchor, member, solo],
      2,
      false,
      [group],
      new Set([anchor.workspace_id!]),
    );
    expect(markup).toContain("cmux-sidebar-group-header is-multi-selected");
    expect(markup).toContain("cmux-sidebar-row is-selected");
  });
});
