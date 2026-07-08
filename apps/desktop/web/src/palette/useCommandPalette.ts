import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  CommandPaletteCommand,
} from "../components/CommandRow";
import type { CommandPaletteResolvedSearchMatch } from "../components/CommandPalette";
import { host } from "../host/host";
import { useSession } from "../hooks/useSession";
import {
  buildCommandCatalog,
  dispatchCommand,
  searchableTexts,
  type CommandContext,
} from "./commandCatalog";
import {
  COMMAND_PALETTE_COMMANDS_PREFIX,
  listScope,
  queryForMatching,
  type CommandPaletteListScope,
} from "./listScope";
import {
  buildSwitcherEntries,
  switcherCandidateCommandIds,
  type SwitcherEntry,
} from "./switcherEntries";
import { useFocusedPanelId } from "../session/focusedPane";
import { planIntent } from "./intentPlan";

/**
 * D4 — the live command-palette host. Composes the ported pure modules
 * (`commandCatalog`, `switcherEntries`, `listScope`) and the Rust search bridge
 * (`command_palette_search`, the D2 orchestrator) into an openable overlay, and
 * emits the exact `commands` + `matches` shapes the pure {@link CommandPalette}
 * list renderer already consumes.
 *
 * Scope mirrors canonical cmux: an empty query (or any query NOT starting with
 * `>`) is the **switcher** (fuzzy workspace jump); a `>`-prefixed query is the
 * **commands** list. `queryForMatching` strips the prefix before matching.
 *
 * Search runs through the Rust orchestrator when Tauri is present (the parity
 * path); in a plain-browser dev runtime it falls back to a client-side filter so
 * the overlay is still visible and navigable.
 */

/** One corpus record, matching the Rust `CorpusEntryInput` (camelCase). */
interface CorpusEntry {
  commandId: string;
  rank: number;
  title: string;
  searchableTexts: string[];
}

/** One resolved match from the Rust bridge (`SearchMatch`, camelCase). */
interface SearchMatch {
  commandId: string;
  score: number;
  titleMatchIndices: number[];
}

export interface UseCommandPalette {
  visible: boolean;
  query: string;
  scope: CommandPaletteListScope;
  /** Commands available for the current scope, keyed by `id` for the list. */
  commands: CommandPaletteCommand[];
  /** Resolved matches (ordered), in the list renderer's snake_case shape. */
  matches: CommandPaletteResolvedSearchMatch[];
  selectedIndex: number;
  open: (initialQuery?: string) => void;
  close: () => void;
  setQuery: (query: string) => void;
  move: (delta: number) => void;
  activateAt: (index: number) => void;
}

const RESULT_LIMIT = 50;

/** Client-side fallback matcher used only when the Tauri bridge is absent. */
function fallbackSearch(
  corpus: CorpusEntry[],
  matchingQuery: string,
  resultLimit: number,
): SearchMatch[] {
  const query = matchingQuery.toLowerCase().trim();
  if (query === "") {
    return corpus
      .slice(0, resultLimit)
      .map((entry) => ({ commandId: entry.commandId, score: 0, titleMatchIndices: [] }));
  }
  const matches: SearchMatch[] = [];
  for (const entry of corpus) {
    const haystack = entry.searchableTexts.join(" ").toLowerCase();
    if (!haystack.includes(query)) {
      continue;
    }
    const titleIdx = entry.title.toLowerCase().indexOf(query);
    const titleMatchIndices =
      titleIdx >= 0 ? Array.from({ length: query.length }, (_, i) => titleIdx + i) : [];
    matches.push({
      commandId: entry.commandId,
      score: titleIdx >= 0 ? 100 : 50,
      titleMatchIndices,
    });
  }
  return matches.slice(0, resultLimit);
}

/** Host actions the palette cannot reach through the session layer. */
export interface CommandPaletteHostActions {
  /** Collapse/expand the workspace sidebar (App-owned view state). */
  toggleSidebar?: () => void;
}

export function useCommandPalette(
  hostActions?: CommandPaletteHostActions,
): UseCommandPalette {
  const {
    snapshot,
    activeLayout,
    workspaces,
    selectedWorkspaceIndex,
    selectWorkspace,
    newWorkspace,
    closeWorkspace,
    split,
    equalizeDividers,
    renameWorkspace,
    setWorkspacePinned,
  } = useSession();

  const [visible, setVisible] = useState(false);
  const [query, setQueryState] = useState("");
  const [matches, setMatches] = useState<CommandPaletteResolvedSearchMatch[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);

  // Monotonic guard so a slow async search cannot overwrite a newer one.
  const searchSeq = useRef(0);

  const scope = listScope(query);
  const matchingQuery = queryForMatching(query);

  // Focused-panel id (≡ the canonical surface id): the tracked focused pane
  // (pointer-down/focus capture on Workspace's pane wrappers), revalidated
  // against the active layout with a first-leaf fallback. It feeds BOTH the
  // catalog context (`hasFocusedPanel` row gating) and the intent planner. It
  // must be identical in both, or `dispatchCommand` inside `activateAt` rebuilds
  // the catalog without a row the list displayed and activation silently nulls.
  const activePanelId = useFocusedPanelId(activeLayout);

  // The ONE catalog context, shared by the catalog build below and the
  // `dispatchCommand` call inside `activateAt`. The two MUST be identical
  // field-for-field: `dispatchCommand` rebuilds the catalog, so any ctx drift
  // makes a displayed row's `when` gate drop it at activation time and the
  // click silently no-ops. Keys mirror the canonical host's computation off
  // tabManager.selectedWorkspace (ContentView.swift:6154-6161):
  // - workspaceHasCustomName ≡ customTitle != nil (:6159) — both sides hold
  //   the trimmed-nonempty-or-absent invariant via A6, so presence == has
  //   custom name.
  // - workspaceShouldPin ≡ !workspace.isPinned for the live selected
  //   workspace (:6161); snapshot is_pinned is Some(true)|None, so undefined
  //   reads as unpinned.
  const commandContext = useMemo<CommandContext>(() => {
    const selected = workspaces[selectedWorkspaceIndex];
    return {
      hasWorkspace: workspaces.length > 0,
      workspaceName: selected?.custom_title ?? selected?.process_title ?? null,
      hasFocusedPanel: activePanelId !== undefined,
      workspaceHasCustomName: selected?.custom_title !== undefined,
      workspaceShouldPin: !(selected?.is_pinned === true),
    };
  }, [workspaces, selectedWorkspaceIndex, activePanelId]);

  // Build the scope's commands, the search corpus, the switcher candidate ids,
  // and (switcher) the id→workspace map for activation. The pure modules do all
  // the parity work; this only marshals their output.
  const { commands, corpus, candidateCommandIds, switcherById } = useMemo(() => {
    if (scope === "commands") {
      const built: CommandPaletteCommand[] = buildCommandCatalog(commandContext).map((d) => ({
        id: d.id,
        rank: d.rank,
        title: d.title,
        subtitle: d.subtitle,
        shortcut_hint: d.shortcutHint ?? null,
        kind_label: d.kindLabel,
        keywords: d.keywords,
        dismiss_on_run: d.dismissOnRun,
      }));
      return {
        commands: built,
        corpus: toCorpus(built),
        candidateCommandIds: [] as string[],
        switcherById: new Map<string, SwitcherEntry>(),
      };
    }

    // switcher scope
    const tabs = snapshot?.windows[0]?.tab_manager ?? {
      workspaces: [],
      selected_workspace_index: 0,
    };
    const entries = buildSwitcherEntries(tabs);
    const built: CommandPaletteCommand[] = entries.map((entry) => ({
      id: entry.id,
      rank: entry.rank,
      title: entry.title,
      subtitle: entry.subtitle,
      shortcut_hint: null,
      kind_label: entry.kindLabel,
      keywords: entry.keywords,
      dismiss_on_run: entry.dismissOnRun,
    }));
    const byId = new Map(entries.map((entry) => [entry.id, entry]));
    return {
      commands: built,
      corpus: toCorpus(built),
      candidateCommandIds: switcherCandidateCommandIds(entries),
      switcherById: byId,
    };
  }, [scope, snapshot, commandContext]);

  // Run the search whenever the query, scope, corpus, or visibility changes.
  useEffect(() => {
    if (!visible) {
      return;
    }
    const seq = (searchSeq.current += 1);
    let cancelled = false;

    void host
      .invoke<SearchMatch[]>("command_palette_search", {
        request: {
          scope,
          query: matchingQuery,
          candidateCommandIds,
          corpus,
          usageHistory: {},
          queryIsEmpty: matchingQuery === "",
          historyTimestamp: 0,
          resultLimit: RESULT_LIMIT,
        },
      })
      .catch(() => fallbackSearch(corpus, matchingQuery, RESULT_LIMIT))
      .then((found) => {
        if (cancelled || seq !== searchSeq.current) {
          return;
        }
        const resolved: CommandPaletteResolvedSearchMatch[] = found.map((m) => ({
          command_id: m.commandId,
          score: m.score,
          title_match_indices: m.titleMatchIndices,
        }));
        setMatches(resolved);
        setSelectedIndex((prev) =>
          resolved.length === 0 ? 0 : Math.min(prev, resolved.length - 1),
        );
      });

    return () => {
      cancelled = true;
    };
  }, [visible, scope, matchingQuery, corpus, candidateCommandIds]);

  const open = useCallback((initialQuery = "") => {
    setQueryState(initialQuery);
    setSelectedIndex(0);
    setMatches([]);
    setVisible(true);
  }, []);

  const close = useCallback(() => {
    setVisible(false);
    setQueryState("");
    setMatches([]);
    setSelectedIndex(0);
  }, []);

  const setQuery = useCallback((next: string) => {
    setQueryState(next);
    // Re-anchor the cursor to the top on any query change.
    setSelectedIndex(0);
  }, []);

  const move = useCallback(
    (delta: number) => {
      setSelectedIndex((prev) => {
        if (matches.length === 0) {
          return 0;
        }
        return Math.max(0, Math.min(prev + delta, matches.length - 1));
      });
    },
    [matches.length],
  );

  const activateAt = useCallback(
    (index: number) => {
      const match = matches[index];
      if (!match) {
        return;
      }
      if (scope === "switcher") {
        const entry = switcherById.get(match.command_id);
        if (entry) {
          const targetIndex = workspaces.findIndex(
            (w) => w.workspace_id === entry.target.workspaceId,
          );
          if (targetIndex >= 0) {
            selectWorkspace(targetIndex);
          }
        }
      } else {
        // Commands scope: resolve the intent through the shared dispatch path
        // (with the SAME ctx the catalog was built from — see commandContext),
        // decide what it does with the pure planner, and execute. Unmapped
        // kinds still log so the wiring never silently no-ops.
        const dispatch = dispatchCommand(match.command_id, commandContext);
        if (dispatch) {
          const plan = planIntent(dispatch.intent.kind, {
            selectedWorkspaceIndex,
            workspaceCount: workspaces.length,
            activePanelId,
            selectedWorkspaceId: workspaces[selectedWorkspaceIndex]?.workspace_id,
            selectedWorkspaceIsPinned:
              workspaces[selectedWorkspaceIndex]?.is_pinned === true,
          });
          switch (plan.type) {
            case "newWorkspace":
              newWorkspace();
              break;
            case "closeWorkspace":
              closeWorkspace(plan.index);
              break;
            case "selectWorkspace":
              selectWorkspace(plan.index);
              break;
            case "split":
              split(plan.panelId, plan.orientation, plan.insertFirst);
              break;
            case "equalizeDividers":
              equalizeDividers();
              break;
            case "setWorkspacePinned":
              setWorkspacePinned(plan.index, plan.pinned);
              break;
            case "renameWorkspace":
              renameWorkspace(plan.index, plan.title);
              break;
            case "toggleSidebar":
              hostActions?.toggleSidebar?.();
              break;
            case "copyText":
              void copyTextToClipboard(plan.text);
              break;
            case "none":
              break;
            case "unhandled":
              // eslint-disable-next-line no-console
              console.info(
                "[command-palette] unhandled intent",
                match.command_id,
                dispatch.intent.kind,
              );
              break;
          }
        }
      }
      close();
    },
    [
      matches,
      scope,
      switcherById,
      workspaces,
      selectedWorkspaceIndex,
      activePanelId,
      commandContext,
      selectWorkspace,
      newWorkspace,
      closeWorkspace,
      split,
      equalizeDividers,
      setWorkspacePinned,
      renameWorkspace,
      hostActions,
      close,
    ],
  );

  // Global open shortcuts (only while hidden — the overlay owns key handling
  // once visible). Ctrl/Cmd+K → switcher; Ctrl/Cmd+Shift+P → commands.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (visible) {
        return;
      }
      if (!(event.metaKey || event.ctrlKey)) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        open("");
      } else if (key === "p" && event.shiftKey) {
        event.preventDefault();
        open(COMMAND_PALETTE_COMMANDS_PREFIX);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visible, open]);

  return {
    visible,
    query,
    scope,
    commands,
    matches,
    selectedIndex,
    open,
    close,
    setQuery,
    move,
    activateAt,
  };
}

/**
 * Copy `text` to the clipboard without ever throwing out of the palette's
 * activation path (canonical NSPasteboard cannot fail; here WebView2 /
 * non-secure contexts can reject the async API or omit `navigator.clipboard`
 * entirely, so fall back to the `execCommand` textarea trick and swallow).
 */
async function copyTextToClipboard(text: string): Promise<void> {
  if (navigator.clipboard !== undefined) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall through to the execCommand fallback.
    }
  }
  execCommandCopy(text);
}

/** Legacy-path copy via an off-screen readonly textarea selection. */
function execCommandCopy(text: string): void {
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  try {
    textarea.select();
    document.execCommand("copy");
  } catch (error) {
    // eslint-disable-next-line no-console
    console.error("[command-palette] clipboard copy failed", error);
  } finally {
    textarea.remove();
  }
}

/** Map display commands to the search corpus shape the Rust bridge expects. */
function toCorpus(commands: CommandPaletteCommand[]): CorpusEntry[] {
  return commands.map((command) => ({
    commandId: command.id,
    rank: command.rank,
    title: command.title,
    searchableTexts: searchableTexts({
      title: command.title,
      subtitle: command.subtitle,
      keywords: command.keywords,
    }),
  }));
}
