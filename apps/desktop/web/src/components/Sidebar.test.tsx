import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";
import type { BadgeVisibilitySettings } from "../sidebar/badges";
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
  showWorkspaceDescription = false,
  badgeVisibilitySettings?: Partial<BadgeVisibilitySettings>,
  showProgress = true,
  showLog = true,
  showCustomMetadata = true,
): string {
  return renderToStaticMarkup(
    <SidebarView
      collapsed={collapsed}
      showWorkspaceDescription={showWorkspaceDescription}
      badgeVisibilitySettings={badgeVisibilitySettings}
      showProgress={showProgress}
      showLog={showLog}
      showCustomMetadata={showCustomMetadata}
      workspaces={workspaces}
      workspaceGroups={workspaceGroups}
      selectedWorkspaceIndex={selectedWorkspaceIndex}
      onNewWorkspace={noop}
      onNewBrowserWorkspace={noop}
      onOpenWorkspaceFolder={noop}
      onOpenWorkspacePullRequests={noop}
      onSelectWorkspace={noop}
      onCloseWorkspace={noop}
      onReorderWorkspace={noop}
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
    // The reused webviews Icon emits an <svg> for the browser control, plus
    // one per row icon and pin toggle.
    expect(count(markup, "<svg")).toBe(5);
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
    expect(markup).toContain("cmux-sidebar-browser");
    expect(markup).toContain('aria-label="New browser workspace"');
    expect(markup).toContain("Open folder as workspace");
    expect(markup).toContain("Open workspace pull requests");
  });

  test("renders a custom sidebar picker when custom sidebars are available", () => {
    const markup = renderToStaticMarkup(
      <SidebarView
        collapsed={false}
        workspaces={[ws({ process_title: "only" })]}
        selectedWorkspaceIndex={0}
        customSidebars={[
          {
            name: "ops",
            kind: "json",
            path: "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.json",
          },
        ]}
        onNewWorkspace={noop}
        onNewBrowserWorkspace={noop}
        onOpenWorkspaceFolder={noop}
        onOpenWorkspacePullRequests={noop}
        onSelectWorkspace={noop}
        onCloseWorkspace={noop}
        onReorderWorkspace={noop}
        onRenameWorkspace={noop}
        onSetWorkspacePinned={noop}
        onToggleGroupCollapsed={noop}
      />,
    );

    expect(markup).toContain('aria-label="Select custom sidebar"');
    expect(markup).toContain(">ops</option>");
    expect(markup).toContain(">Workspaces<");
  });

  test("selected custom sidebar mode replaces the workspace list with host content", () => {
    const markup = renderToStaticMarkup(
      <SidebarView
        collapsed={false}
        workspaces={[ws({ process_title: "workspace" })]}
        selectedWorkspaceIndex={0}
        selectedCustomSidebar={{
          name: "ops",
          kind: "json",
          path: "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.json",
        }}
        customSidebarContent={<div>Custom host</div>}
        customSidebars={[
          {
            name: "ops",
            kind: "json",
            path: "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.json",
          },
        ]}
        onNewWorkspace={noop}
        onSelectWorkspace={noop}
        onCloseWorkspace={noop}
        onReorderWorkspace={noop}
        onRenameWorkspace={noop}
        onSetWorkspacePinned={noop}
        onToggleGroupCollapsed={noop}
      />,
    );

    expect(markup).toContain('aria-label="Custom sidebar"');
    expect(markup).toContain(">Back<");
    expect(markup).toContain(">Reload<");
    expect(markup).toContain("Safe default policy");
    expect(markup).toContain("No capability manifest");
    expect(markup).toContain("Custom host");
    expect(markup).not.toContain("cmux-sidebar-list");
    expect(markup).not.toContain(">workspace<");
  });

  test("selected custom sidebar mode surfaces manifest trust and denied methods", () => {
    const markup = renderToStaticMarkup(
      <SidebarView
        collapsed={false}
        workspaces={[ws({ process_title: "workspace" })]}
        selectedWorkspaceIndex={0}
        selectedCustomSidebar={{
          name: "ops",
          kind: "swift",
          path: "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.swift",
        }}
        customSidebarContent={<div>Custom host</div>}
        customSidebars={[
          {
            name: "ops",
            kind: "swift",
            path: "C:\\Users\\User\\.config\\cmux\\sidebars\\ops.swift",
            manifest: {
              valid: true,
              trusted: true,
              requested_methods: [
                "browser.open",
                "sidebar.list",
                "workspace.select",
              ],
              allowed_requested_methods: ["sidebar.list", "workspace.select"],
              denied_requested_methods: ["browser.open"],
              enforced: true,
            },
          },
        ]}
        onNewWorkspace={noop}
        onSelectWorkspace={noop}
        onCloseWorkspace={noop}
        onReorderWorkspace={noop}
        onRenameWorkspace={noop}
        onSetWorkspacePinned={noop}
        onToggleGroupCollapsed={noop}
      />,
    );

    expect(markup).toContain("Some requests denied");
    expect(markup).toContain("3 requested");
    expect(markup).toContain("2 allowed");
    expect(markup).toContain("Denied: browser.open");
    expect(markup).toContain("cmux-sidebar-custom-manifest-warn");
  });

  test("renders the browser workspace button immediately before plus", () => {
    const markup = render([ws({ process_title: "only" })]);

    expect(markup.indexOf('aria-label="New browser workspace"')).toBeLessThan(
      markup.indexOf('aria-label="New workspace"'),
    );
    expect(markup).toContain('title="New browser workspace"');
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

  test("workspace descriptions render only when enabled", () => {
    const rows = [ws({ process_title: "a", custom_description: "Build is red" })];
    const hidden = render(rows);
    expect(hidden).not.toContain("cmux-sidebar-row-description");

    const visible = render(rows, 0, false, undefined, undefined, true);
    expect(visible).toContain("cmux-sidebar-row-description");
    expect(visible).toContain(">Build is red<");
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

  test("renders git and pull-request badges from session workspace fields", () => {
    const markup = render([
      ws({
        process_title: "repo",
        layout: { type: "pane", pane: { panel_ids: ["surface-1"] } },
        listening_ports: [5173],
        panel_git_branches: [
          { panel_id: "surface-1", branch: "feature", is_dirty: true },
        ],
        panel_pull_requests: [
          {
            panel_id: "surface-1",
            number: 12,
            label: "owner/repo",
            url: "https://github.com/owner/repo/pull/12",
            status: "open",
            branch: "feature",
            is_stale: false,
          },
        ],
      }),
    ]);

    expect(markup).toContain("cmux-sidebar-row-badges");
    expect(markup).toContain("feature*");
    expect(markup).toContain("owner/repo #12");
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-badge-status">open</span>',
    );
    expect(markup).toContain('href="http://localhost:5173"');
    expect(markup).toContain(":5173");
  });

  test("renders remote SSH status badges from session workspace fields", () => {
    const markup = render([
      ws({
        process_title: "remote",
        remote: {
          enabled: true,
          state: "connected",
          connected: true,
          transport: "ssh",
          destination: "dev.example.com",
          port: 22,
          local_proxy_port: 31337,
          persistent_daemon_slot: "ssh-workspace-1",
          has_ssh_options: true,
          daemon: {
            state: "ready",
            capabilities: ["browser-proxy"],
          },
          proxy: {
            state: "ready",
            host: "127.0.0.1",
            port: 31337,
            schemes: ["socks5"],
            url: "socks5://127.0.0.1:31337",
          },
          detected_ports: [5173],
          forwarded_ports: [5173],
          conflicted_ports: [],
          active_terminal_sessions: 1,
        },
      }),
    ]);

    expect(markup).toContain("cmux-sidebar-row-badge--remote");
    expect(markup).toContain("SSH dev.example.com");
    expect(markup).toContain(
      '<span class="cmux-sidebar-row-badge-status">connected</span>',
    );
    expect(markup).toContain("socks5://127.0.0.1:31337");
  });

  test("sidebar badge visibility settings gate rendered detail badges", () => {
    const rows = [
      ws({
        process_title: "repo",
        layout: { type: "pane", pane: { panel_ids: ["surface-1"] } },
        listening_ports: [5173],
        panel_git_branches: [
          { panel_id: "surface-1", branch: "feature", is_dirty: true },
        ],
        panel_pull_requests: [
          {
            panel_id: "surface-1",
            number: 12,
            label: "owner/repo",
            url: "https://github.com/owner/repo/pull/12",
            status: "open",
            branch: "feature",
            is_stale: false,
          },
        ],
        remote: {
          enabled: true,
          state: "connected",
          connected: true,
          transport: "ssh",
          destination: "dev.example.com",
          port: 22,
          local_proxy_port: 31337,
          persistent_daemon_slot: "ssh-workspace-1",
          has_ssh_options: false,
          daemon: { state: "ready", capabilities: [] },
          proxy: {
            state: "ready",
            host: "127.0.0.1",
            port: 31337,
            schemes: ["socks5"],
            url: "socks5://127.0.0.1:31337",
          },
          detected_ports: [],
          forwarded_ports: [],
          conflicted_ports: [],
          active_terminal_sessions: 1,
        },
      }),
    ];

    const hidden = render(rows, 0, false, undefined, undefined, false, {
      hideAllDetails: true,
    });
    expect(hidden).not.toContain("feature*");
    expect(hidden).not.toContain("owner/repo #12");
    expect(hidden).not.toContain("SSH dev.example.com");
    expect(hidden).not.toContain(":5173");

    const selective = render(rows, 0, false, undefined, undefined, false, {
      showBranchDirectory: false,
      showPullRequests: false,
      showSsh: false,
      showPorts: true,
    });
    expect(selective).not.toContain("feature*");
    expect(selective).not.toContain("owner/repo #12");
    expect(selective).not.toContain("SSH dev.example.com");
    expect(selective).toContain(":5173");
  });

  test("renders unread indicators for workspaces with unread panels", () => {
    const markup = render([
      ws({
        process_title: "a",
        panel_unreads: [{ panel_id: "surface-1", is_unread: true }],
      }),
      ws({ process_title: "b" }),
    ]);
    expect(markup).toContain("cmux-sidebar-row is-selected is-unread");
    expect(count(markup, "cmux-sidebar-unread-dot")).toBe(1);
    expect(markup).toContain('aria-label="Unread workspace"');
  });

  test("renders a grouped unread indicator when any group member is unread", () => {
    const anchor = ws({ process_title: "anchor" });
    const member = ws({
      process_title: "member",
      panel_unreads: [{ panel_id: "surface-1", is_unread: true }],
    });
    const gid = mintId();
    anchor.group_id = gid;
    member.group_id = gid;
    const group: SessionWorkspaceGroupSnapshot = {
      id: gid,
      name: "Backend",
      is_collapsed: true,
      anchor_workspace_id: anchor.workspace_id,
    };
    const markup = render([anchor, member], 0, false, [group]);
    expect(markup).toContain("cmux-sidebar-group-header is-collapsed is-selected is-unread");
    expect(markup).toContain("cmux-sidebar-unread-dot");
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

  test("renders workspace progress unless progress details are disabled", () => {
    const workspace = ws({
      sidebar_progress: { value: 0.5, label: "Building" },
    });

    const visible = render([workspace]);
    expect(visible).toContain("cmux-sidebar-row-progress");
    expect(visible).toContain("Building 50%");

    const disabled = render(
      [workspace],
      0,
      false,
      undefined,
      undefined,
      false,
      undefined,
      false,
    );
    expect(disabled).not.toContain("cmux-sidebar-row-progress");

    const hiddenDetails = render(
      [workspace],
      0,
      false,
      undefined,
      undefined,
      false,
      { hideAllDetails: true },
    );
    expect(hiddenDetails).not.toContain("cmux-sidebar-row-progress");
  });

  test("renders sidebar status and log details behind their settings gates", () => {
    const workspace = ws({
      sidebar_status_entries: [
        { key: "deploy", value: "ready", priority: 90, updated_at: 10 },
      ],
      sidebar_metadata_entries: [
        {
          key: "task",
          value: "review",
          icon: "text:CTX",
          url: "https://example.test/pr",
          format: "markdown",
          priority: 50,
          updated_at: 12,
        },
      ],
      sidebar_metadata_blocks: [
        { key: "notes", markdown: "handoff ready", priority: 10, updated_at: 13 },
      ],
      sidebar_log_entries: [{ level: "info", message: "ship it", created_at: 11 }],
    });

    const visible = render([workspace]);
    expect(visible).toContain("cmux-sidebar-row-status");
    expect(visible).toContain("deploy");
    expect(visible).toContain("ready");
    expect(visible).toContain("cmux-sidebar-row-meta");
    expect(visible).toContain("task");
    expect(visible).toContain("review");
    expect(visible).toContain("cmux-sidebar-row-meta-block");
    expect(visible).toContain("handoff ready");
    expect(visible).toContain("cmux-sidebar-row-log");
    expect(visible).toContain("ship it");

    const noLog = render(
      [workspace],
      0,
      false,
      undefined,
      undefined,
      false,
      undefined,
      true,
      false,
      true,
    );
    expect(noLog).toContain("cmux-sidebar-row-status");
    expect(noLog).toContain("cmux-sidebar-row-meta");
    expect(noLog).not.toContain("cmux-sidebar-row-log");

    const noMetadata = render(
      [workspace],
      0,
      false,
      undefined,
      undefined,
      false,
      undefined,
      true,
      true,
      false,
    );
    expect(noMetadata).not.toContain("cmux-sidebar-row-status");
    expect(noMetadata).not.toContain("cmux-sidebar-row-meta");
    expect(noMetadata).toContain("cmux-sidebar-row-log");

    const hiddenDetails = render(
      [workspace],
      0,
      false,
      undefined,
      undefined,
      false,
      { hideAllDetails: true },
    );
    expect(hiddenDetails).not.toContain("cmux-sidebar-row-status");
    expect(hiddenDetails).not.toContain("cmux-sidebar-row-meta");
    expect(hiddenDetails).not.toContain("cmux-sidebar-row-log");
  });
});
