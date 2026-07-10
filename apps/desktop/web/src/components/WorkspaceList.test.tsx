import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  renderItems,
  type WorkspaceGroup,
  type WorkspaceRow,
} from "../sidebar/renderItems";
import {
  clickModifiers,
  renameActionForKey,
  SIDEBAR_DRAG_CLEAR_EVENTS,
  WorkspaceList,
} from "./WorkspaceList";

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
  customColor?: string,
): WorkspaceRow {
  return { id, groupId: group, isPinned: pinned, customColor };
}

function group(
  id: string,
  anchor: string,
  collapsed: boolean,
  pinned = false,
  customColor?: string,
): WorkspaceGroup {
  return {
    id,
    name: "G",
    isCollapsed: collapsed,
    isPinned: pinned,
    anchorWorkspaceId: anchor,
    customColor,
  };
}

function groupsMap(groups: WorkspaceGroup[]): Map<string, WorkspaceGroup> {
  return new Map(groups.map((g) => [g.id, g]));
}

function count(haystack: string, needle: string): number {
  return haystack.split(needle).length - 1;
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
    // The chevron is an interactive collapse-toggle button (canonical: a
    // separate tap target from the header body), not a decorative span.
    expect(markup).toContain(
      '<button type="button" class="cmux-sidebar-group-chevron"',
    );
    expect(markup).toContain('aria-label="Collapse group"');
    expect(markup).not.toContain('cmux-sidebar-group-chevron" aria-hidden');
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
    // Collapsed chevron offers the expand action.
    expect(markup).toContain('aria-label="Expand group"');
    // Member count still reflects both members even while collapsed.
    expect(markup).toContain(
      '<span class="cmux-sidebar-group-count">2</span>',
    );
    // No workspace rows are rendered for the collapsed members.
    expect(markup).not.toContain('<li class="cmux-sidebar-row');
    expect(markup).not.toContain(`data-workspace-id="${member}"`);
  });

  test("labels rows via titleForWorkspace, falling back to the id", () => {
    const { solo, member } = UUID;
    const items = renderItems(
      [row(solo, undefined, false), row(member, undefined, false)],
      groupsMap([]),
    );
    const titles = new Map<string, string>([[solo, "zsh"]]);
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={(id) => titles.get(id) ?? id}
      />,
    );
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-label">zsh</span>',
    );
    // No title known for the member -> the id itself.
    expect(markup).toContain(
      `<span class="cmux-sidebar-row-label">${member}</span>`,
    );
  });

  test("renders an optional workspace description under the title", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "zsh"}
        descriptionForWorkspace={() => "Build is red"}
      />,
    );
    expect(markup).toContain("cmux-sidebar-row-text has-description");
    expect(markup).toContain('<span class="cmux-sidebar-row-label">zsh</span>');
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-description">Build is red</span>',
    );
  });

  test("renders workspace progress as a compact progressbar", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        progressForWorkspace={() => ({ value: 0.375, label: "Building" })}
      />,
    );

    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-valuenow="38"');
    expect(markup).toContain("Building 38%");
    expect(markup).toContain("width:38%");
  });

  test("renders workspace status and log details", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        detailsForWorkspace={() => ({
          statusEntries: [{ key: "build", value: "green", priority: 80 }],
          metadataEntries: [
            {
              key: "task",
              value: "review",
              icon: "text:CTX",
              url: "https://example.test/pr",
              format: "markdown",
              priority: 70,
            },
          ],
          metadataBlocks: [
            { key: "notes", markdown: "**Ready** to ship", priority: 10 },
          ],
          logEntries: [{ level: "info", message: "ship it", createdAt: 42 }],
        })}
      />,
    );

    expect(markup).toContain("cmux-sidebar-row-status");
    expect(markup).toContain("build");
    expect(markup).toContain("green");
    expect(markup).toContain("cmux-sidebar-row-meta");
    expect(markup).toContain("task");
    expect(markup).toContain("review");
    expect(markup).toContain("href=\"https://example.test/pr\"");
    expect(markup).toContain("cmux-sidebar-row-meta-block");
    expect(markup).toContain("notes");
    expect(markup).toContain("**Ready** to ship");
    expect(markup).toContain("cmux-sidebar-row-log");
    expect(markup).toContain("[info]");
    expect(markup).toContain("ship it");
  });

  test("wrapWorkspaceTitles marks row labels as wrappable", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "A very long workspace title"}
        wrapWorkspaceTitles
      />,
    );

    expect(markup).toContain("cmux-sidebar-row-text can-wrap-title");
    expect(markup).toContain(">A very long workspace title<");
  });

  test("renders branch and pull-request badges for workspace rows", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "zsh"}
        badgesForWorkspace={() => ({
          branchSummaryText: "feature*",
          badges: [
            {
              kind: "branch",
              id: "branch:feature",
              label: "feature*",
              tone: "secondary",
              name: "feature",
              isDirty: true,
            },
            {
              kind: "pullRequest",
              id: "owner/repo#12|https://github.com/owner/repo/pull/12",
              label: "owner/repo #12",
              tone: "secondaryStale",
              statusLabel: "open",
              status: "open",
              url: "https://github.com/owner/repo/pull/12",
              number: 12,
              repoLabel: "owner/repo",
              isStale: true,
            },
            {
              kind: "shellActivity",
              id: "shell-activity:running",
              label: "shell",
              tone: "secondary",
              status: "running",
              statusLabel: "running",
              runningPanelCount: 1,
              title: "Running command in surface-1",
            },
            {
              kind: "port",
              id: "port:5173",
              label: ":5173",
              tone: "secondary",
              port: 5173,
              url: "http://localhost:5173",
            },
          ],
        })}
      />,
    );

    expect(markup).toContain("cmux-sidebar-row-badges");
    expect(markup).toContain(
      'class="cmux-sidebar-row-badge cmux-sidebar-row-badge--branch"',
    );
    expect(markup).toContain("feature*");
    expect(markup).toContain(
      'class="cmux-sidebar-row-badge cmux-sidebar-row-badge--pull-request is-stale"',
    );
    expect(markup).toContain('href="https://github.com/owner/repo/pull/12"');
    expect(markup).toContain("owner/repo #12");
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-badge-status">open</span>',
    );
    expect(markup).toContain(
      'class="cmux-sidebar-row-badge cmux-sidebar-row-badge--port"',
    );
    expect(markup).toContain(
      'class="cmux-sidebar-row-badge cmux-sidebar-row-badge--shell-activity"',
    );
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-badge-status">running</span>',
    );
    expect(markup).toContain('href="http://localhost:5173"');
    expect(markup).toContain(":5173");
  });

  test("inline branch layout renders the compact branch summary once", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "zsh"}
        branchLayout="inline"
        badgesForWorkspace={() => ({
          branchSummaryText: "main* | feature",
          badges: [
            {
              kind: "branch",
              id: "branch:main",
              label: "main*",
              tone: "secondary",
              name: "main",
              isDirty: true,
            },
            {
              kind: "branch",
              id: "branch:feature",
              label: "feature",
              tone: "secondary",
              name: "feature",
              isDirty: false,
            },
            {
              kind: "port",
              id: "port:5173",
              label: ":5173",
              tone: "secondary",
              port: 5173,
              url: "http://localhost:5173",
            },
          ],
        })}
      />,
    );

    expect(markup).toContain("main* | feature");
    expect(count(markup, "cmux-sidebar-row-badge--branch")).toBe(1);
    expect(markup).toContain(":5173");
  });

  test("pull-request badges render as inert pills when clickability is disabled", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "zsh"}
        makePullRequestsClickable={false}
        badgesForWorkspace={() => ({
          branchSummaryText: null,
          badges: [
            {
              kind: "pullRequest",
              id: "owner/repo#12|https://github.com/owner/repo/pull/12",
              label: "owner/repo #12",
              tone: "secondary",
              statusLabel: "open",
              status: "open",
              url: "https://github.com/owner/repo/pull/12",
              number: 12,
              repoLabel: "owner/repo",
              isStale: false,
            },
          ],
        })}
      />,
    );

    expect(markup).toContain("owner/repo #12");
    expect(markup).toContain("cmux-sidebar-row-badge--pull-request");
    expect(markup).not.toContain('href="https://github.com/owner/repo/pull/12"');
  });

  test("close affordance renders only when canCloseWorkspaces", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const without = renderToStaticMarkup(<WorkspaceList items={items} />);
    expect(without).not.toContain("cmux-sidebar-row-close");
    const withClose = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        canCloseWorkspaces
        titleForWorkspace={() => "zsh"}
      />,
    );
    expect(withClose).toContain("cmux-sidebar-row-close");
    expect(withClose).toContain('aria-label="Close zsh"');
  });

  test("selected workspace close affordance is visible without hover", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        canCloseWorkspaces
        selectedWorkspaceIds={new Set([solo])}
        titleForWorkspace={() => "zsh"}
      />,
    );

    expect(markup).toContain(
      'class="cmux-sidebar-row-close is-visible"',
    );
    expect(markup).toContain('aria-label="Close zsh"');
  });

  test("rows and group headers become draggable when reorder is enabled", () => {
    const { gid, anchor, member, solo } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, false),
      row(solo, undefined, false),
    ];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, false)]));
    const markup = renderToStaticMarkup(
      <WorkspaceList items={items} onReorderWorkspace={() => {}} />,
    );

    expect(markup).toContain(`data-group-id="${gid}"`);
    expect(markup).toContain(`data-workspace-id="${member}"`);
    expect(markup).toContain(`data-workspace-id="${solo}"`);
    // One header + two visible rows = three draggable list items.
    expect((markup.match(/draggable="true"/g) ?? []).length).toBe(3);
  });

  test("sidebar drag cleanup covers cancelled and outside-drop paths", () => {
    expect(SIDEBAR_DRAG_CLEAR_EVENTS.windowImmediate).toEqual([
      "dragend",
      "blur",
    ]);
    expect(SIDEBAR_DRAG_CLEAR_EVENTS.windowDeferred).toEqual(["drop"]);
    expect(SIDEBAR_DRAG_CLEAR_EVENTS.documentImmediate).toEqual([
      "visibilitychange",
    ]);
  });

  test("group custom colors tint the header and member rows", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const items = renderItems(
      tabs,
      groupsMap([group(gid, anchor, false, false, "#33AA77")]),
    );
    const markup = renderToStaticMarkup(<WorkspaceList items={items} />);

    expect(markup).toContain('class="cmux-sidebar-group-header has-accent');
    expect(markup).toContain('class="cmux-sidebar-row has-accent"');
    expect(markup).toContain('--cmux-sidebar-accent:#33AA77');
    expect((markup.match(/cmux-sidebar-accent-pill/g) ?? []).length).toBe(2);
  });

  test("workspace custom color overrides inherited group tint", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, false, "#1565C0"),
    ];
    const items = renderItems(
      tabs,
      groupsMap([group(gid, anchor, false, false, "#C0392B")]),
    );
    const markup = renderToStaticMarkup(<WorkspaceList items={items} />);

    expect(markup).toContain(
      `style="--cmux-sidebar-accent:#C0392B" data-group-id="${gid}" data-collapsed="false"`,
    );
    expect(markup).toContain(
      `style="--cmux-sidebar-accent:#1565C0" data-workspace-id="${member}"`,
    );
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

    // The pinned member row carries is-pinned and renders a pin glyph.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row is-pinned"[^>]*data-workspace-id="${member}"`,
      ),
    );
    expect(markup).toContain("cmux-sidebar-workspace-pin");
    // The selected solo row carries is-selected and aria-selected.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row is-selected"[^>]*data-workspace-id="${solo}"[^>]*aria-selected="true"`,
      ),
    );
    // The member row is not selected.
    expect(markup).toMatch(
      new RegExp(
        `data-workspace-id="${member}"[^>]*aria-selected="false"`,
      ),
    );
  });

  test("default render shows the label span and no rename input", () => {
    const { solo, member } = UUID;
    const items = renderItems(
      [row(solo, undefined, false), row(member, undefined, false)],
      groupsMap([]),
    );
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={() => "zsh"}
        onRenameWorkspace={() => {}}
      />,
    );
    expect(markup).toContain("cmux-sidebar-row-label");
    expect(markup).not.toContain("cmux-sidebar-row-rename");
  });

  test("defaultEditingWorkspaceId renders the rename input prefilled with the row title", () => {
    const { solo, member } = UUID;
    const items = renderItems(
      [row(solo, undefined, false), row(member, undefined, false)],
      groupsMap([]),
    );
    const titles = new Map<string, string>([[solo, "zsh"]]);
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={(id) => titles.get(id) ?? id}
        onRenameWorkspace={() => {}}
        defaultEditingWorkspaceId={solo}
      />,
    );
    // The editing row swaps its label span for a prefilled text input (static
    // markup serializes defaultValue as the value attribute).
    expect(markup).toContain('class="cmux-sidebar-row-rename"');
    expect(markup).toContain('value="zsh"');
    expect(markup).toContain('aria-label="Rename zsh"');
    expect(markup).not.toContain(
      '<span class="cmux-sidebar-row-label">zsh</span>',
    );
    // The other row keeps its plain label.
    expect(markup).toContain(
      `<span class="cmux-sidebar-row-label">${member}</span>`,
    );
  });

  test("renameActionForKey: Enter commits, Escape cancels, others pass through", () => {
    expect(renameActionForKey("Enter")).toBe("commit");
    expect(renameActionForKey("Escape")).toBe("cancel");
    expect(renameActionForKey("a")).toBe(null);
    expect(renameActionForKey("Tab")).toBe(null);
  });

  test("pin toggle labels: Unpin on pinned rows, Pin on unpinned rows", () => {
    const { solo, member } = UUID;
    const items = renderItems(
      [row(solo, undefined, true), row(member, undefined, false)],
      groupsMap([]),
    );
    const titles = new Map<string, string>([
      [solo, "zsh"],
      [member, "vim"],
    ]);
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={(id) => titles.get(id) ?? id}
        onSetWorkspacePinned={() => {}}
      />,
    );
    // Canonical label oracle: "Pin Workspace"/"Unpin Workspace"
    // (WorkspaceActionDispatcher.swift:166-170).
    expect(markup).toContain('aria-label="Unpin zsh"');
    expect(markup).toContain('title="Unpin Workspace"');
    expect(markup).toContain('aria-label="Pin vim"');
    expect(markup).toContain('title="Pin Workspace"');
  });

  test("pin toggle is absent when onSetWorkspacePinned is not provided", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, true)], groupsMap([]));
    const markup = renderToStaticMarkup(<WorkspaceList items={items} />);
    expect(markup).not.toContain("cmux-sidebar-row-pin");
  });

  test("renders an opened workspace context menu with shared row actions", () => {
    const { solo, member } = UUID;
    const items = renderItems(
      [row(solo, undefined, true), row(member, undefined, false)],
      groupsMap([]),
    );
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        titleForWorkspace={(id) => (id === solo ? "Pinned" : "Other")}
        canCloseWorkspaces
        onRenameWorkspace={() => {}}
        onSetWorkspacePinned={() => {}}
        onContextMenuAction={() => {}}
        defaultContextMenuTarget={{ kind: "workspace", workspaceId: solo }}
      />,
    );

    expect(markup).toContain('role="menu"');
    expect(markup).toContain('aria-label="More actions for Pinned"');
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain("New Workspace");
    expect(markup).toContain("Rename Workspace");
    expect(markup).toContain("Unpin Workspace");
    expect(markup).toContain("Close Workspace");
    expect(markup).toContain("Close Other Workspaces");
  });

  test("workspace context menu disables unavailable row actions", () => {
    const { solo } = UUID;
    const items = renderItems([row(solo, undefined, false)], groupsMap([]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        onContextMenuAction={() => {}}
        defaultContextMenuTarget={{ kind: "workspace", workspaceId: solo }}
      />,
    );

    expect(markup).toContain("Rename Workspace");
    expect(markup).toContain("Pin Workspace");
    expect(markup).toContain("Close Workspace");
    // Rename, pin, close, and close-others are disabled. New Workspace stays
    // enabled through the shared sidebar action sink.
    expect((markup.match(/disabled=""/g) ?? []).length).toBe(4);
  });

  test("group header markup carries no pin toggle", () => {
    // Collapsed group: only the header renders (member rows suppressed), so
    // the whole markup must be pin-toggle-free even with the handler wired
    // (group pin is setWorkspaceGroupPinned — a different op, out of scope).
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, true)]));
    const markup = renderToStaticMarkup(
      <WorkspaceList items={items} onSetWorkspacePinned={() => {}} />,
    );
    expect(markup).toContain("cmux-sidebar-group-header");
    expect(markup).not.toContain("cmux-sidebar-row-pin");
  });

  test("renders an opened group context menu with group actions", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, false)]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        canCloseWorkspaces
        onToggleGroupCollapsed={() => {}}
        onContextMenuAction={() => {}}
        defaultContextMenuTarget={{ kind: "group", groupId: gid }}
      />,
    );

    expect(markup).toContain('aria-label="More actions for group G"');
    expect(markup).toContain("New Workspace");
    expect(markup).toContain("Collapse Group");
    expect(markup).toContain("Close Group");
    expect(markup).not.toContain("Rename Workspace");
  });

  test("clickModifiers: shift, ctrl, meta, none", () => {
    const ev = (
      shiftKey: boolean,
      ctrlKey: boolean,
      metaKey: boolean,
    ) => ({ shiftKey, ctrlKey, metaKey });
    expect(clickModifiers(ev(true, false, false))).toEqual({
      shift: true,
      toggle: false,
    });
    // Ctrl is the Windows chord for canonical Cmd.
    expect(clickModifiers(ev(false, true, false))).toEqual({
      shift: false,
      toggle: true,
    });
    // metaKey kept for parity on hosts that surface it.
    expect(clickModifiers(ev(false, false, true))).toEqual({
      shift: false,
      toggle: true,
    });
    expect(clickModifiers(ev(false, false, false))).toEqual({
      shift: false,
      toggle: false,
    });
  });

  test("multi-selected rows draw is-multi-selected, active row keeps is-selected only", () => {
    const { solo, member, anchor } = UUID;
    const items = renderItems(
      [
        row(anchor, undefined, false),
        row(member, undefined, false),
        row(solo, undefined, false),
      ],
      groupsMap([]),
    );
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={new Set([anchor])}
        // The active row is ALSO in the multi-selection — canonical isActive
        // precedence (SidebarAppearanceSupport.swift:312-336) means it draws
        // is-selected only, never both classes.
        multiSelectedWorkspaceIds={new Set([anchor, member])}
      />,
    );
    // The non-active multi-selected row.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row is-multi-selected"[^>]*data-workspace-id="${member}"[^>]*aria-selected="true"`,
      ),
    );
    // The active row: is-selected only, no double class.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row is-selected"[^>]*data-workspace-id="${anchor}"`,
      ),
    );
    // The unselected row carries neither.
    expect(markup).toMatch(
      new RegExp(
        `<li class="cmux-sidebar-row"[^>]*data-workspace-id="${solo}"[^>]*aria-selected="false"`,
      ),
    );
  });

  test("group header draws is-multi-selected when its anchor is multi-selected", () => {
    const { gid, anchor, member, solo } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, false),
      row(solo, undefined, false),
    ];
    const items = renderItems(tabs, groupsMap([group(gid, anchor, false)]));
    const markup = renderToStaticMarkup(
      <WorkspaceList
        items={items}
        selectedWorkspaceIds={new Set([solo])}
        multiSelectedWorkspaceIds={new Set([anchor])}
      />,
    );
    expect(markup).toMatch(
      /<li class="cmux-sidebar-group-header is-multi-selected"[^>]*aria-selected="true"/,
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
