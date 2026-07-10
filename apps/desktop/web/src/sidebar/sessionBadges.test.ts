import { describe, expect, test } from "bun:test";

import type {
  SessionWorkspaceLayoutSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import {
  badgesForSessionWorkspace,
  orderedPanelIdsFromLayout,
} from "./sessionBadges";

function pane(panelIds: string[]): SessionWorkspaceLayoutSnapshot {
  return { type: "pane", pane: { panel_ids: panelIds } };
}

function split(
  first: SessionWorkspaceLayoutSnapshot,
  second: SessionWorkspaceLayoutSnapshot,
): SessionWorkspaceLayoutSnapshot {
  return {
    type: "split",
    split: {
      orientation: "horizontal",
      divider_position: 0.5,
      first,
      second,
    },
  };
}

function workspace(
  overrides: Partial<SessionWorkspaceSnapshot> = {},
): SessionWorkspaceSnapshot {
  return {
    process_title: "Terminal",
    layout: pane(["p1"]),
    ...overrides,
  };
}

describe("session badge adapter", () => {
  test("orderedPanelIdsFromLayout walks pane leaves left-to-right", () => {
    expect(
      orderedPanelIdsFromLayout(split(pane(["a", "b"]), split(pane(["c"]), pane(["d"])))),
    ).toEqual(["a", "b", "c", "d"]);
  });

  test("projects panel branch and pull-request snapshots into badge descriptors", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        layout: pane(["p2", "p1"]),
        panel_git_branches: [
          { panel_id: "p1", branch: "feature", is_dirty: false },
          { panel_id: "p2", branch: "main", is_dirty: true },
        ],
        panel_pull_requests: [
          {
            panel_id: "p1",
            number: 42,
            label: "owner/repo",
            url: "https://github.com/owner/repo/pull/42",
            status: "open",
            branch: "feature",
            is_stale: false,
          },
        ],
      }),
    );

    expect(result.badges.map((badge) => badge.label)).toEqual([
      "main*",
      "feature",
      "owner/repo #42",
    ]);
  });

  test("uses the workspace branch fallback only when no panel branch reports", () => {
    const fallback = badgesForSessionWorkspace(
      workspace({
        git_branch: { branch: "fallback", is_dirty: true },
      }),
    );
    expect(fallback.branchSummaryText).toBe("fallback*");

    const panelWins = badgesForSessionWorkspace(
      workspace({
        git_branch: { branch: "fallback", is_dirty: true },
        panel_git_branches: [
          { panel_id: "p1", branch: "panel", is_dirty: false },
        ],
      }),
    );
    expect(panelWins.branchSummaryText).toBe("panel");
  });

  test("filters panel PRs whose branch no longer matches the panel branch", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        panel_git_branches: [
          { panel_id: "p1", branch: "main", is_dirty: false },
        ],
        panel_pull_requests: [
          {
            panel_id: "p1",
            number: 7,
            label: "owner/repo",
            url: "https://github.com/owner/repo/pull/7",
            status: "open",
            branch: "feature",
            is_stale: false,
          },
        ],
      }),
    );

    expect(result.badges.map((badge) => badge.kind)).toEqual(["branch"]);
  });

  test("keeps the first duplicate panel snapshot", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        panel_git_branches: [
          { panel_id: "p1", branch: "first", is_dirty: false },
          { panel_id: "p1", branch: "second", is_dirty: true },
        ],
      }),
    );

    expect(result.branchSummaryText).toBe("first");
  });

  test("projects workspace listening ports into sidebar badge descriptors", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        listening_ports: [5173, 3000, 5173],
      }),
    );

    expect(result.badges).toEqual([
      {
        kind: "port",
        id: "port:3000",
        label: ":3000",
        tone: "secondary",
        port: 3000,
        url: "http://localhost:3000",
      },
      {
        kind: "port",
        id: "port:5173",
        label: ":5173",
        tone: "secondary",
        port: 5173,
        url: "http://localhost:5173",
      },
    ]);
  });

  test("falls back to panel listening-port facts when aggregate ports are absent", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        panel_listening_ports: [
          { panel_id: "p1", ports: [8080] },
          { panel_id: "p2", ports: [3000, 8080] },
        ],
      }),
    );

    expect(result.badges.map((badge) => badge.label)).toEqual([":3000", ":8080"]);
  });

  test("projects panel shell activity snapshots into sidebar runtime badges", () => {
    const result = badgesForSessionWorkspace(
      workspace({
        layout: pane(["p1", "p2"]),
        panel_shell_activity: [
          { panel_id: "p1", state: "promptIdle", updated_at: 1 },
          { panel_id: "p2", state: "commandRunning", updated_at: 2 },
        ],
      }),
    );

    expect(result.badges.at(-1)).toMatchObject({
      kind: "shellActivity",
      label: "shell",
      statusLabel: "running",
      runningPanelCount: 1,
    });
  });

  test("projects workspace remote snapshots into SSH sidebar badges", () => {
    const result = badgesForSessionWorkspace(
      workspace({
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
    );

    expect(result.badges.map((badge) => badge.kind)).toEqual(["remote"]);
    expect(result.badges[0]).toMatchObject({
      kind: "remote",
      label: "SSH dev.example.com",
      status: "connected",
      statusLabel: "connected",
      title:
        "SSH dev.example.com | state: connected | proxy: socks5://127.0.0.1:31337 | local proxy: 31337 | forwarded: 5173 | terminals: 1 | custom SSH options",
    });
  });
});
