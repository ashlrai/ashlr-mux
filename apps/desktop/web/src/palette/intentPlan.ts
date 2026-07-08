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
// - copyWorkspaceID / copySurfaceID copy the canonical single-line formats
//   (`WorkspaceSurfaceIdentifierClipboardText.swift:85-92` `workspace_id=<id>`,
//   `:36-43` `surface_id=<id>`; one line, no trailing newline). Ids are copied
//   VERBATIM from the port's own session state (lowercase Rust uuids /
//   `surface-<n>` panel ids) — no uppercase transform, so copied values
//   round-trip against the port. The port's panel id IS the canonical surface
//   id (`surfaceId = panelContext.panelId`,
//   `ContentViewIdentifierCopyCommands.swift:123`). Missing workspace/panel →
//   `none` (canonical beeps, :101-104).
// - Deliberately UNHANDLED copy/window kinds (fall through to `unhandled`):
//   copyWorkspaceIDAndRef (the ref line is the point,
//   `ContentViewIdentifierCopyCommands.swift:100-106`; the port has no v2 ref
//   registry), copyPaneID (the bonsplit pane NODE uuid, :127-133; the port's
//   layout snapshot carries no pane identity), copyIdentifiers (hardcodes
//   includeRefs:true, :170-183 — a ref-less/pane-less block would be an
//   invented format), toggleFullScreen (webview lacks
//   `core:window:allow-set-fullscreen` capability; needs a src-tauri lane),
//   newWindow (port is single-window).

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
  | { type: "copyText"; text: string }
  | { type: "none" }
  | { type: "unhandled" };

export interface IntentPlanContext {
  /** Index of the selected workspace (clamped by the caller, 0-based). */
  selectedWorkspaceIndex: number;
  /** Number of workspaces in the first window. */
  workspaceCount: number;
  /**
   * The pane splits target (also the canonical surface id), or undefined when
   * no splittable pane exists.
   */
  activePanelId?: string;
  /**
   * workspace_id of the selected workspace, verbatim from the snapshot;
   * undefined when absent.
   */
  selectedWorkspaceId?: string;
}

/** Decide what `kind` does given the current session shape. */
export function planIntent(
  kind: CommandIntentKind,
  ctx: IntentPlanContext,
): IntentPlan {
  const { selectedWorkspaceIndex, workspaceCount, activePanelId, selectedWorkspaceId } =
    ctx;
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
    case "copyWorkspaceID":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? { type: "copyText", text: `workspace_id=${selectedWorkspaceId}` }
        : { type: "none" };
    case "copySurfaceID":
      return activePanelId !== undefined
        ? { type: "copyText", text: `surface_id=${activePanelId}` }
        : { type: "none" };
    case "equalizeSplits":
      return { type: "equalizeDividers" };
    case "toggleSidebar":
      return { type: "toggleSidebar" };
    default:
      return { type: "unhandled" };
  }
}
