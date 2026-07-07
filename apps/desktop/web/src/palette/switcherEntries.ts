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
// DATA-AVAILABILITY GAP (the central D6 constraint):
//   * Workspace metadata degrades to `{ directories: [current_directory] }`.
//     `SessionWorkspaceSnapshot` has no gitBranch / listeningPorts /
//     customDescription (those are Lane A's new fields), so
//     `commandPaletteWorkspaceSearchMetadata` (5483-5494) ports to
//     directories-only; branches/ports/description stay empty.
//   * SURFACE rows are NOT emitted. The snapshot exposes only a pane `layout`
//     with a 2-value `surface_kind` ("terminal"|"agent"), NOT panel titles,
//     per-panel directories/branches/ports, or the 9-case `PanelType`
//     (5311-5331, 5508-5551). Faithful surface rows are impossible until Lane A
//     adds panel fields, so `includeSurfaces` mirrors the Swift signature but is
//     reserved: no `switcher.surface.*` entries are produced yet. Emitting
//     reduced-fidelity surfaces now would diverge from macOS goldens.
//
// PARITY NUANCE: the workspace display-name fallback is "Workspace" (8177), NOT
// the sidebar's "Terminal" (`components/Sidebar.tsx:22`). The strings differ and
// are golden-visible, so `Sidebar`'s helper is intentionally NOT reused.

import type { SessionTabManagerSnapshot } from "@cmux/core-types";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import { searchableTexts } from "./commandCatalog";
import { trimWhitespaceAndNewlines } from "./listScope";
import { switcherSearchKeywords, type SwitcherSearchMetadata } from "./switcherIndex";

/** Injected environment for the pure producer. */
export interface SwitcherEntriesEnv {
  /** Home directory for path standardization (`switcherIndex` `env.homeDir`). */
  homeDir?: string;
  /**
   * Mirror of Swift `includeSurfaces` (5249). RESERVED: surface rows require
   * panel data absent from the current snapshot, so no surface entries are
   * emitted regardless. See the data-availability gap in the file header.
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
 * `commandPaletteWorkspaceSearchMetadata` (5483-5494), degraded to
 * directories-only for the current snapshot (see the file-header data gap).
 * Swift always passes a single-element `[currentDirectory]`; the indexer drops
 * empty/whitespace values, so an absent directory contributes no keywords.
 */
function workspaceSearchMetadata(
  workspace: SessionWorkspaceSnapshot,
): SwitcherSearchMetadata {
  return { directories: [workspace.current_directory ?? ""] };
}

/**
 * Builds the workspace switcher entries in selected-hoisted order. Surface rows
 * are not emitted (see the file-header data gap); `env.includeSurfaces` is
 * reserved for when Lane A adds panel fields.
 *
 * Port of `commandPaletteSwitcherEntries` (5249-5358) restricted to the
 * single-window fallback + workspace loop.
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
