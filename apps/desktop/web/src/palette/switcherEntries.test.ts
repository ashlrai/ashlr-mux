// Oracle tests for switcherEntries.ts, pinned to
// `commandPaletteSwitcherEntries` (`Sources/ContentView.swift:5249-5358`) and
// its helpers (5419-5433 ordering, 8171-8178 display name, 5483-5494 metadata).
// The keyword sub-oracle reuses the algorithm proven in switcherIndex.test.ts.

import { describe, expect, test } from "bun:test";

import type {
  SessionTabManagerSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import {
  buildSwitcherEntries,
  switcherCandidateCommandIds,
  switcherCorpus,
  workspaceDisplayName,
} from "./switcherEntries";

function workspace(partial: Partial<SessionWorkspaceSnapshot>): SessionWorkspaceSnapshot {
  return {
    process_title: "",
    layout: null,
    ...partial,
  };
}

function snapshot(
  workspaces: SessionWorkspaceSnapshot[],
  selectedIndex?: number,
): SessionTabManagerSnapshot {
  return { workspaces, selected_workspace_index: selectedIndex };
}

describe("workspaceDisplayName — ContentView.swift:8171-8178", () => {
  test("custom_title wins (trimmed with whitespacesAndNewlines)", () => {
    expect(workspaceDisplayName(workspace({ custom_title: "  X ", process_title: "zsh" }))).toBe(
      "X",
    );
  });

  test("blank custom falls back to process_title (trimmed)", () => {
    expect(
      workspaceDisplayName(workspace({ custom_title: "   ", process_title: " zsh " })),
    ).toBe("zsh");
  });

  test("both blank falls back to 'Workspace' (NOT the sidebar's 'Terminal')", () => {
    expect(workspaceDisplayName(workspace({ custom_title: "", process_title: "" }))).toBe(
      "Workspace",
    );
    expect(workspaceDisplayName(workspace({}))).toBe("Workspace");
  });
});

describe("selected-hoist ordering — ContentView.swift:5419-5433", () => {
  test("selected_workspace_index moves that workspace to the front, others stable", () => {
    const entries = buildSwitcherEntries(
      snapshot(
        [
          workspace({ workspace_id: "AAA", process_title: "a" }),
          workspace({ workspace_id: "BBB", process_title: "b" }),
          workspace({ workspace_id: "CCC", process_title: "c" }),
          workspace({ workspace_id: "DDD", process_title: "d" }),
        ],
        2,
      ),
    );
    expect(entries.map((e) => e.target.workspaceId)).toEqual(["CCC", "AAA", "BBB", "DDD"]);
  });

  test("missing / out-of-range index leaves order unchanged", () => {
    const wss = [
      workspace({ workspace_id: "AAA", process_title: "a" }),
      workspace({ workspace_id: "BBB", process_title: "b" }),
    ];
    expect(
      buildSwitcherEntries(snapshot(wss)).map((e) => e.target.workspaceId),
    ).toEqual(["AAA", "BBB"]);
    expect(
      buildSwitcherEntries(snapshot(wss, 9)).map((e) => e.target.workspaceId),
    ).toEqual(["AAA", "BBB"]);
  });
});

describe("id + rank — ContentView.swift:5274, 5306", () => {
  test("ids are switcher.workspace.<lowercased id>; ranks contiguous from 0", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({ workspace_id: "AbC-123", process_title: "a" }),
        workspace({ workspace_id: "DEF", process_title: "b" }),
      ]),
    );
    expect(entries.map((e) => e.id)).toEqual([
      "switcher.workspace.abc-123",
      "switcher.workspace.def",
    ]);
    expect(entries.map((e) => e.rank)).toEqual([0, 1]);
  });

  test("workspaces with no workspace_id are skipped, and rank stays contiguous", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({ workspace_id: "AAA", process_title: "a" }),
        workspace({ process_title: "no-id" }),
        workspace({ workspace_id: "", process_title: "blank-id" }),
        workspace({ workspace_id: "BBB", process_title: "b" }),
      ]),
    );
    expect(entries.map((e) => e.id)).toEqual([
      "switcher.workspace.aaa",
      "switcher.workspace.bbb",
    ]);
    expect(entries.map((e) => e.rank)).toEqual([0, 1]);
  });
});

describe("entry shape — ContentView.swift:5288-5305", () => {
  test("kindLabel, subtitle, dismissOnRun, target", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([workspace({ workspace_id: "AAA", process_title: "zsh" })]),
    );
    expect(entry.title).toBe("zsh");
    expect(entry.kindLabel).toBe("Workspace");
    // Single-window: subtitle is the bare base, no ` • <window>` suffix (5410).
    expect(entry.subtitle).toBe("Workspace");
    expect(entry.dismissOnRun).toBe(true);
    expect(entry.target).toEqual({ workspaceId: "AAA" });
  });
});

describe("keyword pipeline integration — ContentView.swift:5275-5285 + switcherIndex", () => {
  test("current_directory adds the directory context family + path token", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          current_directory: "/Users/x/dev",
        }),
      ]),
    );
    // Base keywords first (5276-5285), then metadata context words + tokens.
    expect(entry.keywords).toEqual([
      "workspace",
      "switch",
      "go",
      "open",
      "Phoenix",
      "directory",
      "dir",
      "cwd",
      "path",
      "/Users/x/dev",
    ]);
  });

  test("empty-metadata safety: no current_directory → base list only", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([workspace({ workspace_id: "AAA", process_title: "Phoenix" })]),
    );
    expect(entry.keywords).toEqual(["workspace", "switch", "go", "open", "Phoenix"]);
  });

  test("custom_description adds the description keyword family + tokens", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          custom_description: "fix login redirect",
        }),
      ]),
    );
    expect(entry.keywords).toEqual([
      "workspace",
      "switch",
      "go",
      "open",
      "Phoenix",
      "description",
      "descriptions",
      "notes",
      "note",
      "fix login redirect",
      "fix",
      "login",
      "redirect",
    ]);
  });

  test("workspace git branches add the branch keyword family", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          git_branch: { branch: "main", is_dirty: false },
        }),
      ]),
    );
    expect(entry.keywords).toEqual([
      "workspace",
      "switch",
      "go",
      "open",
      "Phoenix",
      "branch",
      "git",
      "main",
    ]);
  });

  test("workspace panel branches win over the workspace fallback branch", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          git_branch: { branch: "main", is_dirty: false },
          panel_git_branches: [
            { panel_id: "surface-1", branch: "feature/login", is_dirty: true },
            { panel_id: "surface-2", branch: "feature/login", is_dirty: false },
            { panel_id: "surface-3", branch: "release", is_dirty: false },
          ],
        }),
      ]),
    );
    expect(entry.keywords).toEqual([
      "workspace",
      "switch",
      "go",
      "open",
      "Phoenix",
      "branch",
      "git",
      "feature/login",
      "release",
    ]);
  });

  test("workspace listening ports add the port keyword family", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          listening_ports: [3000, 5173, 3000],
        }),
      ]),
    );
    expect(entry.keywords).toEqual([
      "workspace",
      "switch",
      "go",
      "open",
      "Phoenix",
      "port",
      "ports",
      "3000",
      ":3000",
      "5173",
      ":5173",
    ]);
  });

  test("workspace ports fall back to panel port facts when aggregate is absent", () => {
    const [entry] = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          panel_listening_ports: [
            { panel_id: "surface-1", ports: [8080] },
            { panel_id: "surface-2", ports: [8080, 9000] },
          ],
        }),
      ]),
    );
    expect(entry.keywords).toContain(":8080");
    expect(entry.keywords).toContain(":9000");
  });
});

describe("corpus / candidate derivation — command_palette.rs:24-38, 104-116", () => {
  test("candidateCommandIds equals the entry ids in order", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({ workspace_id: "AAA", process_title: "a" }),
        workspace({ workspace_id: "BBB", process_title: "b" }),
      ]),
    );
    expect(switcherCandidateCommandIds(entries)).toEqual([
      "switcher.workspace.aaa",
      "switcher.workspace.bbb",
    ]);
  });

  test("corpus searchableTexts is [title, subtitle, ...keywords] and title matches", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({ workspace_id: "AAA", process_title: "Phoenix", current_directory: "/d" }),
      ]),
    );
    const [corpus] = switcherCorpus(entries);
    expect(corpus.commandId).toBe("switcher.workspace.aaa");
    expect(corpus.rank).toBe(0);
    expect(corpus.searchableTexts[0]).toBe("Phoenix");
    expect(corpus.searchableTexts).toEqual([
      "Phoenix",
      "Workspace",
      ...entries[0].keywords,
    ]);
  });
});

describe("includeSurfaces — ContentView.swift:5311-5331", () => {
  test("no switcher.surface.* entries are emitted (default off)", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "a",
          layout: {
            type: "pane",
            pane: {
              panel_ids: ["surface-1"],
              selected_panel_id: "surface-1",
            },
          },
        }),
      ]),
    );
    expect(entries.some((e) => e.id.startsWith("switcher.surface."))).toBe(false);
  });

  test("includeSurfaces:true emits one row per panel after its workspace row", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          current_directory: "/Users/x/dev/app",
          panel_titles: [{ panel_id: "surface-2", custom_title: "API logs" }],
          layout: {
            type: "pane",
            pane: {
              panel_ids: ["surface-1", "surface-2"],
              selected_panel_id: "surface-2",
            },
          },
        }),
      ]),
      { includeSurfaces: true, homeDir: "/Users/x" },
    );
    expect(entries.map((e) => e.id)).toEqual([
      "switcher.workspace.aaa",
      "switcher.surface.aaa.surface-1",
      "switcher.surface.aaa.surface-2",
    ]);
    expect(entries.map((e) => e.rank)).toEqual([0, 1, 2]);
    expect(entries[1]).toMatchObject({
      title: "Phoenix",
      subtitle: "Surface",
      kindLabel: "Terminal",
      target: { workspaceId: "AAA", panelId: "surface-1" },
    });
    expect(entries[2]).toMatchObject({
      title: "API logs",
      subtitle: "Surface",
      kindLabel: "Terminal",
      target: { workspaceId: "AAA", panelId: "surface-2" },
    });
    expect(entries[1].keywords).toEqual([
      "surface",
      "tab",
      "switch",
      "go",
      "open",
      "Phoenix",
      "terminal",
      "shell",
      "directory",
      "dir",
      "cwd",
      "path",
      "/Users/x/dev/app",
      "~/dev/app",
      "app",
      "Users",
      "x",
      "dev",
    ]);
  });

  test("surface title and kind reflect pane-local markdown/file/browser/agent state", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "",
          layout: {
            type: "split",
            split: {
              orientation: "horizontal",
              divider_position: 0.5,
              first: {
                type: "split",
                split: {
                  orientation: "vertical",
                  divider_position: 0.5,
                  first: {
                    type: "pane",
                    pane: {
                      panel_ids: ["md"],
                      selected_panel_id: "md",
                      surface_kind: "markdown",
                      markdown_file_path: "C:\\work\\README.md",
                    },
                  },
                  second: {
                    type: "pane",
                    pane: {
                      panel_ids: ["file"],
                      selected_panel_id: "file",
                      surface_kind: "file",
                      file_path: "C:\\work\\notes.txt",
                    },
                  },
                },
              },
              second: {
                type: "pane",
                pane: {
                  panel_ids: ["web", "agent"],
                  selected_panel_id: "web",
                  surface_kind: "browser",
                  browser_url: "https://example.test",
                },
              },
            },
          },
        }),
        workspace({
          workspace_id: "BBB",
          process_title: "",
          layout: {
            type: "pane",
            pane: {
              panel_ids: ["ask"],
              selected_panel_id: "ask",
              surface_kind: "agent",
            },
          },
        }),
      ]),
      { includeSurfaces: true },
    );

    expect(entries.map((entry) => [entry.id, entry.title, entry.kindLabel])).toEqual([
      ["switcher.workspace.aaa", "Workspace", "Workspace"],
      ["switcher.surface.aaa.md", "README.md", "Markdown"],
      ["switcher.surface.aaa.file", "notes.txt", "File"],
      ["switcher.surface.aaa.web", "https://example.test", "Browser"],
      ["switcher.surface.aaa.agent", "https://example.test", "Browser"],
      ["switcher.workspace.bbb", "Workspace", "Workspace"],
      ["switcher.surface.bbb.ask", "Agent", "Agent"],
    ]);
  });

  test("surface entries index panel branch metadata with surface-detail tokens", () => {
    const entries = buildSwitcherEntries(
      snapshot([
        workspace({
          workspace_id: "AAA",
          process_title: "Phoenix",
          git_branch: { branch: "main", is_dirty: false },
          panel_git_branches: [
            { panel_id: "surface-1", branch: "feature/api", is_dirty: true },
          ],
          panel_listening_ports: [
            { panel_id: "surface-1", ports: [5173] },
          ],
          layout: {
            type: "pane",
            pane: {
              panel_ids: ["surface-1"],
              selected_panel_id: "surface-1",
            },
          },
        }),
      ]),
      { includeSurfaces: true },
    );
    expect(entries[1].keywords).toEqual([
      "surface",
      "tab",
      "switch",
      "go",
      "open",
      "Phoenix",
      "terminal",
      "shell",
      "branch",
      "git",
      "port",
      "ports",
      "feature/api",
      "feature",
      "api",
      "5173",
      ":5173",
    ]);
  });
});
