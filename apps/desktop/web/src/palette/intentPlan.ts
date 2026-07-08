// Pure command-intent → executable-plan mapping — the first slice of the
// canonical handler registry's side effects (`Sources/ContentView.swift`
// registerHandler blocks). `dispatchCommand` resolves a palette command id to
// a stable `CommandIntentKind`; this module decides WHAT that intent does with
// the current session shape, and the thin host glue (useCommandPalette)
// executes the returned plan against `useSession` / host actions.
//
// Kinds not yet mapped return `{ type: "unhandled" }` — the host logs them so
// the wiring never silently no-ops (same visibility contract the D4 stub had).
// The mapped set grows as the session layer gains the matching commands.
//
// Parity notes:
// - nextWorkspace / previousWorkspace WRAP around the ends
//   (`TabManager.swift:3451-3485`: `(i + 1) % tabs.count` /
//   `(i - 1 + tabs.count) % tabs.count`), and are no-ops with no workspaces.
// - closeWorkspace targets the selected workspace; the session command owns
//   the canonical sole-workspace no-op.
// - terminalSplitRight / terminalSplitDown split the target pane side-by-side
//   ("horizontal") / stacked ("vertical") with the new pane second
//   (insertFirst false), matching the canonical right/down direction map.
//   The target is the host-provided `activePanelId` (first leaf until C4
//   focused-pane tracking lands); no target → no-op plan.
// - equalizeSplits equalizes the whole active-workspace tree
//   (`session_equalize_dividers`, span-count semantics).

import type { SessionSplitOrientation } from "@cmux/core-types";

import type { CommandIntentKind } from "./commandCatalog";

/** An executable decision the host maps 1:1 onto session/host actions. */
export type IntentPlan =
  | { type: "newWorkspace" }
  | { type: "closeWorkspace"; index: number }
  | { type: "selectWorkspace"; index: number }
  | {
      type: "split";
      panelId: string;
      orientation: SessionSplitOrientation;
      insertFirst: boolean;
    }
  | { type: "equalizeDividers" }
  | { type: "toggleSidebar" }
  | { type: "none" }
  | { type: "unhandled" };

export interface IntentPlanContext {
  /** Index of the selected workspace (clamped by the caller, 0-based). */
  selectedWorkspaceIndex: number;
  /** Number of workspaces in the first window. */
  workspaceCount: number;
  /** The pane splits target, or undefined when no splittable pane exists. */
  activePanelId?: string;
}

/** Decide what `kind` does given the current session shape. */
export function planIntent(
  kind: CommandIntentKind,
  ctx: IntentPlanContext,
): IntentPlan {
  const { selectedWorkspaceIndex, workspaceCount, activePanelId } = ctx;
  const hasWorkspaces =
    workspaceCount > 0 &&
    selectedWorkspaceIndex >= 0 &&
    selectedWorkspaceIndex < workspaceCount;

  switch (kind) {
    case "newWorkspace":
      return { type: "newWorkspace" };
    case "closeWorkspace":
      return hasWorkspaces
        ? { type: "closeWorkspace", index: selectedWorkspaceIndex }
        : { type: "none" };
    case "nextWorkspace":
      return hasWorkspaces
        ? {
            type: "selectWorkspace",
            index: (selectedWorkspaceIndex + 1) % workspaceCount,
          }
        : { type: "none" };
    case "previousWorkspace":
      return hasWorkspaces
        ? {
            type: "selectWorkspace",
            index:
              (selectedWorkspaceIndex - 1 + workspaceCount) % workspaceCount,
          }
        : { type: "none" };
    case "terminalSplitRight":
      return activePanelId !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "horizontal",
            insertFirst: false,
          }
        : { type: "none" };
    case "terminalSplitDown":
      return activePanelId !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "vertical",
            insertFirst: false,
          }
        : { type: "none" };
    case "equalizeSplits":
      return { type: "equalizeDividers" };
    case "toggleSidebar":
      return { type: "toggleSidebar" };
    default:
      return { type: "unhandled" };
  }
}
