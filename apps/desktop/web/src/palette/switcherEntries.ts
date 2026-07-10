// Switcher-entry producer — port of the workspace/surface switcher rows the
// macOS host builds in `commandPaletteSwitcherEntries(includeSurfaces:)`
// (`Sources/ContentView.swift:5249-5358`) plus its ordering/naming/metadata
// helpers (5409-5507, 8171-8191). Pure and headless (no React / Tauri / DOM);
// reuses the already-ported keyword indexer (`switcherIndex.ts`) and the
// Swift-exact trim (`listScope.ts`).
//
// SINGLE-WINDOW SIMPLIFICATION: the Windows port is single-window, so Swift's
// multi-window context path (`commandPaletteSwitcherWindowContexts`, 5360-5407)
// collapses to the one fallback context with `windowLabel = nil` (5361-5366).
// Therefore `windowLabel` is always absent → `windowKeywords = []` (5415) and
// the subtitle is `base` with no ` • <window>` suffix (5410). Only the fallback
// path is ported; multi-window is out of scope.
//
// LISTENING-PORT NOTE:
//   * Workspace and surface metadata index session snapshot port facts when the
//     desktop backend reports them. Local terminal panels feed these through the
//     Windows PID-tree scanner; explicit/control-socket port reports share the
//     same snapshot path.
//
// PARITY NUANCE: the workspace display-name fallback is "Workspace" (8177), NOT
// the sidebar's "Terminal" (`components/Sidebar.tsx:22`). The strings differ and
// are golden-visible, so `Sidebar`'s helper is intentionally NOT reused.

import type { SessionTabManagerSnapshot } from "@cmux/core-types";
import type { SessionPaneLayoutSnapshot } from "@cmux/core-types";
import type { SessionWorkspaceLayoutSnapshot } from "@cmux/core-types";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import { normalizeSurfaceKind, type SurfaceKind } from "../session/surfaceUrl";
import { searchableTexts } from "./commandCatalog";
import { trimWhitespaceAndNewlines } from "./listScope";
import { switcherSearchKeywords, type SwitcherSearchMetadata } from "./switcherIndex";

/** Injected environment for the pure producer. */
export interface SwitcherEntriesEnv {
  /** Home directory for path standardization (`switcherIndex` `env.homeDir`). */
  homeDir?: string;
  /**
   * Mirror of Swift `includeSurfaces` (5249). When true, rows for each panel in
   * the selected-hoisted workspace order are emitted after that workspace row.
   */
  includeSurfaces?: boolean;
}

/**
 * The focus target an entry activates — the pure analog of
 * `focusCommandPaletteSwitcherTarget` / `…SurfaceTarget` (5448-5481). The D4
 * host maps this to the actual focus call; no closure is embedded (shared
 * focus path, per the shared-behavior rule).
 */
export interface SwitcherTarget {
  workspaceId: string;
  /** Present only for surface entries (not emitted yet). */
  panelId?: string;
}

/**
 * One switcher row — the headless subset of the Swift `CommandPaletteCommand`
 * built at 5288-5305 (the `() -> Void` action becomes {@link target}).
 */
export interface SwitcherEntry {
  id: string;
  /** Sequential `nextRank` across the flattened entry list (5263/5306/5352). */
  rank: number;
  title: string;
  subtitle: string;
  /** "Workspace" for workspace rows (5294); surface kind otherwise. */
  kindLabel: string | null;
  keywords: string[];
  dismissOnRun: boolean;
  target: SwitcherTarget;
}

/** One corpus record for the D2 search bridge (`command_palette.rs:24-38`). */
export interface SwitcherCorpusEntry {
  commandId: string;
  rank: number;
  title: string;
  searchableTexts: string[];
}

// LOCALIZE: the two default strings below.
const WORKSPACE_DISPLAY_NAME_FALLBACK = "Workspace"; // 8177
const WORKSPACE_KIND_LABEL = "Workspace"; // 5294
const WORKSPACE_SUBTITLE_LABEL = "Workspace"; // 5292 (base; windowLabel nil → no suffix)
const SURFACE_SUBTITLE_LABEL = "Surface"; // 5338-ish base; windowLabel nil → no suffix

const SURFACE_KIND_LABELS: Record<SurfaceKind, string> = {
  terminal: "Terminal",
  agent: "Agent",
  markdown: "Markdown",
  file: "File",
  diff: "Diff",
  "custom-sidebar": "Custom Sidebar",
  browser: "Browser",
};

const SURFACE_KIND_KEYWORDS: Record<SurfaceKind, string[]> = {
  terminal: ["terminal", "shell"],
  agent: ["agent", "ai"],
  markdown: ["markdown", "preview"],
  file: ["file", "editor", "text"],
  diff: ["diff", "review"],
  "custom-sidebar": ["custom", "sidebar", "extension"],
  browser: ["browser", "web"],
};

/**
 * Port of `commandPaletteWorkspaceDisplayName` (8171-8178):
 * `custom_title` (trimmed) → `process_title` (trimmed) → "Workspace". Uses the
 * Swift-exact `whitespacesAndNewlines` trim, NOT JS `.trim()`.
 */
export function workspaceDisplayName(workspace: SessionWorkspaceSnapshot): string {
  const custom = trimWhitespaceAndNewlines(workspace.custom_title ?? "");
  if (custom !== "") {
    return custom;
  }
  const title = trimWhitespaceAndNewlines(workspace.process_title ?? "");
  return title === "" ? WORKSPACE_DISPLAY_NAME_FALLBACK : title;
}

/**
 * Selected-hoisted workspace order — port of
 * `commandPaletteOrderedSwitcherWorkspaces` (5419-5433): the workspace at
 * `selected_workspace_index` moves to index 0; every other workspace keeps its
 * relative order. An out-of-range / missing index leaves the order unchanged.
 */
function orderedWorkspaces(
  snapshot: SessionTabManagerSnapshot,
): SessionWorkspaceSnapshot[] {
  const workspaces = [...snapshot.workspaces];
  const selectedIndex = snapshot.selected_workspace_index;
  if (
    selectedIndex !== undefined &&
    Number.isInteger(selectedIndex) &&
    selectedIndex >= 0 &&
    selectedIndex < workspaces.length
  ) {
    const [selected] = workspaces.splice(selectedIndex, 1);
    workspaces.unshift(selected);
  }
  return workspaces;
}

/**
 * Workspace search metadata — port of
 * `commandPaletteWorkspaceSearchMetadata` (5483-5494), currently ported as
 * directory + description metadata. Swift always passes a single-element
 * `[currentDirectory]`; the indexer drops empty/whitespace values, so an
 * absent directory contributes no keywords.
 */
function workspaceSearchMetadata(
  workspace: SessionWorkspaceSnapshot,
): SwitcherSearchMetadata {
  return {
    directories: [workspace.current_directory ?? ""],
    branches: workspaceBranchKeywords(workspace),
    ports: workspacePortKeywords(workspace),
    description: workspace.custom_description,
  };
}

function branchName(branch: string | undefined): string | undefined {
  const trimmed = trimWhitespaceAndNewlines(branch ?? "");
  return trimmed === "" ? undefined : trimmed;
}

function uniqueBranchNames(branches: Array<string | undefined>): string[] {
  const names: string[] = [];
  const seen = new Set<string>();
  for (const branch of branches) {
    const name = branchName(branch);
    if (name === undefined || seen.has(name)) {
      continue;
    }
    seen.add(name);
    names.push(name);
  }
  return names;
}

function workspaceBranchKeywords(workspace: SessionWorkspaceSnapshot): string[] {
  const panelBranches = uniqueBranchNames(
    (workspace.panel_git_branches ?? []).map((entry) => entry.branch),
  );
  if (panelBranches.length > 0) {
    return panelBranches;
  }
  return uniqueBranchNames([workspace.git_branch?.branch]);
}

function surfaceBranchKeywords(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): string[] {
  const panelBranch = branchName(
    workspace.panel_git_branches?.find((entry) => entry.panel_id === panelId)?.branch,
  );
  return panelBranch !== undefined
    ? [panelBranch]
    : uniqueBranchNames([workspace.git_branch?.branch]);
}

function validPort(port: number | undefined): number | undefined {
  if (port === undefined || !Number.isInteger(port) || port < 1 || port > 65535) {
    return undefined;
  }
  return port;
}

function uniquePorts(ports: Array<number | undefined>): number[] {
  const values: number[] = [];
  const seen = new Set<number>();
  for (const rawPort of ports) {
    const port = validPort(rawPort);
    if (port === undefined || seen.has(port)) {
      continue;
    }
    seen.add(port);
    values.push(port);
  }
  return values;
}

function workspacePortKeywords(workspace: SessionWorkspaceSnapshot): number[] {
  const aggregate = uniquePorts(workspace.listening_ports ?? []);
  if (aggregate.length > 0) {
    return aggregate;
  }
  return uniquePorts(
    [
      ...(workspace.agent_listening_ports ?? []),
      ...(workspace.panel_listening_ports ?? []).flatMap((entry) => entry.ports),
    ],
  );
}

function surfacePortKeywords(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): number[] {
  return uniquePorts(
    workspace.panel_listening_ports?.find((entry) => entry.panel_id === panelId)?.ports ?? [],
  );
}

function panelCustomTitle(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): string | undefined {
  const title = workspace.panel_titles?.find((entry) => entry.panel_id === panelId)?.custom_title;
  const trimmed = trimWhitespaceAndNewlines(title ?? "");
  return trimmed === "" ? undefined : trimmed;
}

function basename(path: string | undefined): string | undefined {
  const trimmed = trimWhitespaceAndNewlines(path ?? "");
  if (trimmed === "") {
    return undefined;
  }
  const normalized = trimmed.replace(/[/\\]+$/g, "");
  if (normalized === "") {
    return trimmed;
  }
  return normalized.split(/[/\\]/).at(-1) ?? normalized;
}

function surfaceDisplayName(
  workspace: SessionWorkspaceSnapshot,
  pane: SessionPaneLayoutSnapshot,
  panelId: string,
  workspaceName: string,
): string {
  const customTitle = panelCustomTitle(workspace, panelId);
  if (customTitle !== undefined) {
    return customTitle;
  }
  const kind = normalizeSurfaceKind(pane.surface_kind);
  switch (kind) {
    case "agent":
      return "Agent";
    case "markdown":
      return basename(pane.markdown_file_path) ?? "Markdown";
    case "file":
      return basename(pane.file_path) ?? "File";
    case "diff":
      return "Diff Viewer";
    case "custom-sidebar":
      return basename(pane.file_path) ?? "Custom Sidebar";
    case "browser": {
      const url = trimWhitespaceAndNewlines(pane.browser_url ?? "");
      return url === "" ? "Browser" : url;
    }
    case "terminal":
      return workspaceName === WORKSPACE_DISPLAY_NAME_FALLBACK ? "Terminal" : workspaceName;
  }
}

function surfaceSearchMetadata(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): SwitcherSearchMetadata {
  return {
    directories: [workspace.current_directory ?? ""],
    branches: surfaceBranchKeywords(workspace, panelId),
    ports: surfacePortKeywords(workspace, panelId),
  };
}

function visitSurfacePanes(
  layout: SessionWorkspaceLayoutSnapshot | null,
  visit: (pane: SessionPaneLayoutSnapshot) => void,
): void {
  if (layout === null) {
    return;
  }
  if (layout.type === "pane") {
    visit(layout.pane);
    return;
  }
  visitSurfacePanes(layout.split.first, visit);
  visitSurfacePanes(layout.split.second, visit);
}

/**
 * Builds switcher entries in selected-hoisted workspace order. Surface rows are
 * emitted after their owning workspace when `env.includeSurfaces` is true.
 *
 * Port of `commandPaletteSwitcherEntries` (5249-5358) restricted to the
 * single-window fallback.
 */
export function buildSwitcherEntries(
  snapshot: SessionTabManagerSnapshot,
  env: SwitcherEntriesEnv = {},
): SwitcherEntry[] {
  const entries: SwitcherEntry[] = [];
  let nextRank = 0;

  for (const workspace of orderedWorkspaces(snapshot)) {
    // Skip workspaces with no id: the entry id can't be
    // `switcher.workspace.<uuid>` without one (5274), and a stable, collision-
    // free id is required for the corpus/candidate contract.
    const workspaceId = workspace.workspace_id;
    if (workspaceId === undefined || workspaceId === "") {
      continue;
    }

    const workspaceName = workspaceDisplayName(workspace);
    const keywords = switcherSearchKeywords(
      // baseKeywords (5276-5285); windowKeywords is [] in single-window.
      ["workspace", "switch", "go", "open", workspaceName],
      workspaceSearchMetadata(workspace),
      "workspace",
      { homeDir: env.homeDir },
    );

    entries.push({
      id: `switcher.workspace.${workspaceId.toLowerCase()}`,
      rank: nextRank,
      title: workspaceName,
      subtitle: WORKSPACE_SUBTITLE_LABEL,
      kindLabel: WORKSPACE_KIND_LABEL,
      keywords,
      dismissOnRun: true,
      target: { workspaceId },
    });
    nextRank += 1;

    if (env.includeSurfaces === true) {
      visitSurfacePanes(workspace.layout, (pane) => {
        const kind = normalizeSurfaceKind(pane.surface_kind);
        for (const panelId of pane.panel_ids) {
          const surfaceName = surfaceDisplayName(workspace, pane, panelId, workspaceName);
          const keywords = switcherSearchKeywords(
            [
              "surface",
              "tab",
              "switch",
              "go",
              "open",
              surfaceName,
              workspaceName,
              ...SURFACE_KIND_KEYWORDS[kind],
            ],
            surfaceSearchMetadata(workspace, panelId),
            "surface",
            { homeDir: env.homeDir },
          );
          entries.push({
            id: `switcher.surface.${workspaceId.toLowerCase()}.${panelId.toLowerCase()}`,
            rank: nextRank,
            title: surfaceName,
            subtitle: SURFACE_SUBTITLE_LABEL,
            kindLabel: SURFACE_KIND_LABELS[kind],
            keywords,
            dismissOnRun: true,
            target: { workspaceId, panelId },
          });
          nextRank += 1;
        }
      });
    }
  }

  return entries;
}

/**
 * The searchable corpus for the switcher scope. Mirrors the D2 contract
 * (`command_palette.rs:24-38`): `searchableTexts = [title, subtitle,
 * ...keywords]`, the same body as command-scope rows
 * (`CommandPaletteCommand.swift:49-51`) — reuses that shared derivation.
 */
export function switcherCorpus(entries: SwitcherEntry[]): SwitcherCorpusEntry[] {
  return entries.map((entry) => ({
    commandId: entry.id,
    rank: entry.rank,
    title: entry.title,
    searchableTexts: searchableTexts(entry),
  }));
}

/**
 * The switcher-scope candidate ids, in entry order. The D2 switcher scope
 * restricts matches to these (`command_palette.rs:104-116` and the
 * `switcher_scope_restricts_to_candidate_ids` test at 203-210).
 */
export function switcherCandidateCommandIds(entries: SwitcherEntry[]): string[] {
  return entries.map((entry) => entry.id);
}
