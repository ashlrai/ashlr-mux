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

describe("includeSurfaces — reserved (data gap, ContentView.swift:5311-5331)", () => {
  test("no switcher.surface.* entries are emitted (default off)", () => {
    const entries = buildSwitcherEntries(
      snapshot([workspace({ workspace_id: "AAA", process_title: "a" })]),
    );
    expect(entries.some((e) => e.id.startsWith("switcher.surface."))).toBe(false);
  });

  test("includeSurfaces:true still emits workspace-only entries (reserved flag)", () => {
    const entries = buildSwitcherEntries(
      snapshot([workspace({ workspace_id: "AAA", process_title: "a" })]),
      { includeSurfaces: true },
    );
    expect(entries.map((e) => e.id)).toEqual(["switcher.workspace.aaa"]);
  });
});
