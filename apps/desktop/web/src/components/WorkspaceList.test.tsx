import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  renderItems,
  type WorkspaceGroup,
  type WorkspaceRow,
} from "../sidebar/renderItems";
import { WorkspaceList } from "./WorkspaceList";

// Fixed UUIDs for deterministic markup assertions.
const UUID = {
  gid: "11111111-1111-1111-1111-111111111111",
  anchor: "22222222-2222-2222-2222-222222222222",
  member: "33333333-3333-3333-3333-333333333333",
  solo: "44444444-4444-4444-4444-444444444444",
} as const;

function row(
  id: string,
  group: string | undefined,
  pinned: boolean,
): WorkspaceRow {
  return { id, groupId: group, isPinned: pinned };
}

function group(
  id: string,
  anchor: string,
  collapsed: boolean,
  pinned = false,
): WorkspaceGroup {
  return {
    id,
    name: "G",
    isCollapsed: collapsed,
    isPinned: pinned,
    anchorWorkspaceId: anchor,
  };
}

function groupsMap(groups: WorkspaceGroup[]): Map<string, WorkspaceGroup> {
  return new Map(groups.map((g) => [g.id, g]));
}

describe("WorkspaceList", () => {
  test("renders a group header (with member count) distinctly from workspace rows", () => {
    const { gid, anchor, member, solo } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, false),
      row(solo, undefined, false),
    ];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, false)]));
    const markup = renderToStaticMarkup(<WorkspaceList items={items} />);

    // A header exists, keyed by group id, and is expanded.
    expect(markup).toContain("cmux-sidebar-group-header");
    expect(markup).toContain(`data-group-id="${gid}"`);
    expect(markup).toContain('data-collapsed="false"');
    // Member count = anchor + member = 2.
    expect(markup).toContain(
      '<span class="cmux-sidebar-group-count">2</span>',
    );
    // The anchor is NOT rendered as its own workspace row (suppressed).
    expect(markup).not.toContain(`data-workspace-id="${anchor}"`);
    // The non-anchor member and the ungrouped solo ARE rows.
    expect(markup).toContain(`data-workspace-id="${member}"`);
    expect(markup).toContain(`data-workspace-id="${solo}"`);
    // Header uses the expanded (down) chevron glyph, rows do not carry it.
    expect(markup).toContain("cmux-sidebar-group-chevron");
    // Structural: an <svg> is emitted from the reused Icon.
    expect(markup).toContain("<svg");
  });

  test("collapsed group hides its member rows and shows a collapsed chevron", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, true)]));
    const markup = renderToStaticMarkup(<WorkspaceList items={items} />);

    expect(markup).toContain("cmux-sidebar-group-header");
    expect(markup).toContain("is-collapsed");
    expect(markup).toContain('data-collapsed="true"');
    expect(markup).toContain('aria-expanded="false"');
    // Member count still reflects both members even while collapsed.
    expect(markup).toContain(
      '<span class="cmux-sidebar-group-count">2</span>',
    );
    // No workspace rows are rendered for the collapsed members.
    expect(markup).not.toContain("cmux-sidebar-workspace-row");
    expect(markup).not.toContain(`data-workspace-id="${member}"`);
  });

  test("applies selected and pinned classes to workspace rows", () => {
    const { gid, anchor, member, solo } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, true), // pinned member
      row(solo, undefined, false),
    ];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, false)]));
    const markup = renderToStaticMarkup(
      <WorkspaceList items={items} selectedWorkspaceIds={new Set([solo])} />,
    );

    // The pinned member row carries is-pinned.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row cmux-sidebar-workspace-row is-pinned"[^>]*data-workspace-id="${member}"`,
      ),
    );
    // The selected solo row carries is-selected and aria-selected.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row cmux-sidebar-workspace-row is-selected"[^>]*data-workspace-id="${solo}"[^>]*aria-selected="true"`,
      ),
    );
    // The member row is not selected.
    expect(markup).toMatch(
      new RegExp(
        `data-workspace-id="${member}"[^>]*aria-selected="false"`,
      ),
    );
  });

  test("marks a group header selected when its anchor is selected, and honors group pin state", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const items = renderItems(
      tabs,
      groupsMap([group(gid, anchor, false, /* pinned */ true)]),
    );
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={new Set([anchor])}
      />,
    );

    expect(markup).toMatch(
      /<li class="cmux-sidebar-group-header is-pinned is-selected"/,
    );
  });
});
