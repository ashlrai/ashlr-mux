// Pure command-intent → executable-plan mapping — the first slice of the
// canonical handler registry's side effects (`Sources/ContentView.swift`
// registerHandler blocks). `dispatchCommand` resolves a palette command id to
// a stable `CommandIntentKind`; this module decides WHAT that intent does with
// the current session shape, and the thin host glue (useCommandPalette)
// executes the returned plan against `useSession` / host actions.
//
// Unknown/future kinds return `{ type: "unhandled" }` — the host logs them so
// new catalog rows never silently no-op. The current static catalog is mapped;
// the fallback is a guardrail for later canonical rows/dynamic extensions.
//
// Parity notes:
// - nextWorkspace / previousWorkspace WRAP around the ends
//   (`TabManager.swift:3451-3485`: `(i + 1) % tabs.count` /
//   `(i - 1 + tabs.count) % tabs.count`), and are no-ops with no workspaces.
// - closeWorkspace targets the selected workspace; the session command owns
//   the canonical sole-workspace no-op.
// - newTerminalTab / closeTab are workspace aliases in the current desktop
//   port: the visible "tab" rows create/close the selected workspace through
//   the same session commands until lane-specific browser/tab state exists.
// - openFolder opens the native folder picker, then creates a fresh workspace
//   rooted at that directory. The workspace metadata carries the chosen
//   `current_directory`, and the terminal bridge uses that as the PTY cwd.
// - openFolderInVSCodeInline opens the native folder picker and delegates to
//   the desktop shell's VS Code launcher. The catalog row is live-gated by the
//   native `code` launcher availability probe so the command is not shown as a
//   dead action when VS Code is absent from PATH.
// - installCLI / uninstallCLI delegate to the desktop shell's managed CLI shim
//   installer. The backend owns the bundled-sidecar resolution and user PATH
//   mutation; the UI only chooses the visible row from native status.
// - makeDefaultTerminal delegates to the Windows shell association layer. The
//   first Windows slice registers cmux as the current-user `ssh://` handler;
//   richer executable/script file-type parity is tracked separately with the
//   external-open launch path.
// - terminalSplitRight / terminalSplitDown split the target pane side-by-side
//   ("horizontal") / stacked ("vertical") with the new pane second
//   (insertFirst false), matching the canonical right/down direction map.
//   The target is the host-provided `activePanelId` (the C4 tracked focused
//   pane — pointer-down/focus capture in Workspace — with a first-leaf
//   fallback when none is focused); no target → no-op plan.
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
// - copyWorkspaceLink / copyPaneLink / copySurfaceLink emit the canonical
//   same-session navigation routes (`CmuxNavigationURLRequest.workspaceLink` /
//   `paneLink` / `surfaceLink`, `CmuxSSHURLRequest.swift:609-626`) using the
//   port's current live ids and the host-provided active callback scheme.
// - copyIdentifiers now mirrors canonical
//   `makeWorkspacePaneSurfaceIdentifiers(..., includeRefs: true)` in the
//   ref-absent case: it emits the workspace/pane/surface id block and simply
//   omits ref lines because the Windows port still lacks the v2 ref registry.
// - openDiffViewer / openDirectoryDiffViewer switch the active pane into the
//   port's diff surface. Canonical cmux launches the standalone diff viewer
//   CLI; the Windows port already exposes a pane-hosted diff surface, so this
//   planner routes the visible workspace-level commands through that existing
//   surface lane instead of leaving them dead.
// - markdownZoomIn / markdownZoomOut / markdownZoomReset target the active
//   markdown pane only. The native markdown host owns the persisted point-size
//   stepping and iframe delivery; the planner just threads through the focused
//   panel id when present.
// - toggleWorkspacePin toggles the SELECTED workspace to `!isPinned`
//   (`ContentView.swift:7785-7795` guards selectedWorkspace else beep, then
//   `WorkspaceActionDispatcher.swift:72-99` — for a single live target the
//   pin decision reduces to `pinned = !anchorWorkspace.isPinned`). The
//   session command owns the canonical A7 pinned-boundary reorder +
//   selection-follows-moved-workspace, so the plan is flag-only here.
// - clearWorkspaceName IS rename-to-empty (`ContentView.swift:7771-7777` →
//   `TabManager.swift:1700-1702`: clearCustomTitle ≡ setCustomTitle(nil);
//   the port's `session_rename_workspace` with an empty title clears
//   custom_title+source, A6 golden). No custom-name guard in the planner —
//   the canonical handler has none either (row visibility is the catalog
//   `when` gate's job), and clearing an already-clear title is a no-op on
//   both sides.
// - moveWorkspaceUp / moveWorkspaceDown route the selected workspace through
//   the single-row reorder lane with raw `selectedWorkspaceIndex ± 1`, matching
//   `moveSelectedWorkspace(by:)` (`ContentView.swift:9191-9198`). The session
//   reorder command owns the canonical pin-boundary clamp and group-anchor
//   routing.
// - moveWorkspaceToTop uses `toIndex = 0` through the same reorder lane. The
//   canonical palette handler calls `moveTabsToTop([workspace.id])`
//   (`ContentView.swift:7824-7830`); in the port we infer the equivalent
//   single-workspace outcome through the existing reorder command, which still
//   preserves group/pin invariants and selection-follows-workspace behavior.
// - closeOtherWorkspaces / closeWorkspacesBelow / closeWorkspacesAbove plan the
//   selected window-order indices and delegate the batch mutation to the
//   session layer. That preserves original-index addressing, selection updates,
//   and group-anchor dissolution on close without racing index shifts.
// - renameWorkspace / editWorkspaceDescription start palette-owned edit flows
//   with the selected workspace's current display title / description,
//   mirroring canonical `beginRenameWorkspaceFlow()` /
//   `beginWorkspaceDescriptionFlow()` (`ContentView.swift:7765-7770`,
//   9277-9326). clearWorkspaceDescription is the same mutation lane saved with
//   an empty draft.
// - newWindow / closeWindow / toggleFullScreen route to native window controls
//   owned by the desktop shell, matching the user-visible command rows even
//   though the React tree itself is not the window owner.
// - reopenPreviousSession restores the session snapshot persisted from the
//   prior app launch through the native session layer's best-effort history
//   file rotation.
// - copyWorkspaceIDAndRef emits the workspace id plus the v2 control-handle ref
//   token for the selected workspace. copyIdentifiers mirrors the control
//   socket's active workspace/pane/surface ids and refs so clipboard handoffs
//   can be pasted directly into control-socket calls.
// - Deliberately excluded from the visible static catalog for now:
//   close-all window teardown / modal confirmation flows
//   (`closeWorkspacesWithConfirmation`, `ContentView.swift:9200-9222`; the
//   session port keeps the final workspace alive and shows no modal prompts).
// - reopenClosedBrowserTab is host-backed by the desktop session layer's
//   recently-closed browser URL stack. The full canonical closed-item menu is
//   still larger than this first slice, but the visible command now has a real
//   backend restoration path.
// - markOldestUnreadAndJumpNext uses panel-unread timestamps when every unread
//   marker in the snapshot carries one; legacy unstamped snapshots fall back to
//   visible unread order. The backend mutation is still the canonical workspace
//   read lane.
// - restartSocketListener routes to the Windows named-pipe control listener
//   composition root. The handler restarts the desktop-owned accept loop; the
//   full macOS recovery policy remains larger than this first real listener
//   lifecycle slice.

import type { SessionSplitOrientation } from "@cmux/core-types";

import type { SwitchableSurfaceKind } from "../session/surfaceUrl";
import type { PaneFocusDirection } from "../session/focusedPane";
import type { SettingsPaneSection } from "../settings/settingsSearchResults";
import type { CommandIntentKind } from "./commandCatalog";
import { paneLink, surfaceLink, workspaceLink } from "./cmuxNavigationLinks";

/** An executable decision the host maps 1:1 onto session/host actions. */
export type IntentPlan =
  | { type: "newWorkspace" }
  | { type: "newBrowserWorkspace" }
  | { type: "reopenClosedBrowserTab" }
  | { type: "installCLI" }
  | { type: "uninstallCLI" }
  | { type: "makeDefaultTerminal" }
  | { type: "closeWorkspace"; index: number }
  | { type: "selectWorkspace"; index: number }
  | { type: "jumpUnread" }
  | { type: "markOldestUnreadAndJumpNext" }
  | {
      type: "split";
      panelId: string;
      orientation: SessionSplitOrientation;
      insertFirst: boolean;
      initialTerminalInput?: string;
      currentDirectory?: string;
    }
  | { type: "equalizeDividers" }
  | { type: "setWorkspacePinned"; index: number; pinned: boolean }
  | { type: "setWorkspaceGroupCollapsed"; groupId: string; collapsed: boolean }
  | {
      type: "setWorkspaceUnread";
      index: number;
      unread: boolean;
      preferredPanelId?: string;
    }
  | { type: "resetWorkspaceColor"; index: number }
  | {
      type: "setSurfaceKind";
      panelId: string;
      kind: SwitchableSurfaceKind | null;
    }
  | { type: "closeWorkspaces"; indexes: number[] }
  | {
      type: "reorderWorkspace";
      index: number;
      toIndex: number;
      usesTopLevelRows?: boolean;
    }
  | { type: "beginRenameWorkspace"; workspaceId: string; title: string }
  | { type: "renameWorkspace"; index: number; title: string }
  | { type: "beginRenameTab"; panelId: string; title: string }
  | { type: "renameTab"; panelId: string; title: string }
  | { type: "setPanelPinned"; panelId: string; pinned: boolean }
  | { type: "setPanelUnread"; panelId: string; unread: boolean }
  | { type: "movePanelToNewWorkspace"; panelId: string }
  | {
      type: "beginWorkspaceDescriptionEdit";
      workspaceId: string;
      title: string;
      description: string;
    }
  | { type: "setWorkspaceDescription"; index: number; description: string }
  | { type: "selectAdjacentPanel"; panelId: string; next: boolean }
  | { type: "focusPanel"; panelId: string }
  | { type: "toggleSplitZoom"; panelId: string }
  | { type: "setLayoutMode"; mode: "canvas" | null }
  | { type: "triggerPanelFlash"; panelId: string }
  | { type: "toggleSidebar" }
  | { type: "toggleFileExplorer" }
  | { type: "setBrowserEnabled"; enabled: boolean }
  | { type: "setMinimalMode"; enabled: boolean }
  | { type: "toggleMatchTerminalBackground" }
  | { type: "openSettings"; section?: SettingsPaneSection; query?: string }
  | { type: "showNotifications" }
  | { type: "openFindInDirectory" }
  | { type: "openFolderInVSCodeInline" }
  | { type: "vscodeServeWebStop" }
  | { type: "vscodeServeWebRestart" }
  | { type: "openWorkspacePullRequests" }
  | { type: "restartControlSocketListener" }
  | { type: "openCmuxSettingsFile" }
  | { type: "openGhosttySettingsFile" }
  | { type: "openFolder" }
  | { type: "restorePreviousLaunch" }
  | { type: "newWindow" }
  | { type: "closeWindow" }
  | { type: "toggleFullScreen" }
  | { type: "openTaskManager" }
  | { type: "markdownZoomIn"; panelId: string }
  | { type: "markdownZoomOut"; panelId: string }
  | { type: "markdownZoomReset"; panelId: string }
  | { type: "clearBrowserHistory"; panelId: string }
  | { type: "clearBrowserNetworkRecords"; panelId: string }
  | { type: "toggleBrowserOmnibar"; panelId: string }
  | { type: "toggleBrowserFocusMode"; panelId: string }
  | { type: "toggleBrowserDeveloperTools"; panelId: string }
  | {
      type: "showBrowserDeveloperTools";
      panelId: string;
      panel: "inspector" | "console" | "react" | "network";
    }
  | {
      type: "splitBrowser";
      panelId: string;
      orientation: SessionSplitOrientation;
      insertFirst: boolean;
    }
  | {
      type: "browserCommand";
      panelId: string;
      command:
        | "back"
        | "forward"
        | "reload"
        | "focusAddressBar"
        | "openDefault"
        | "zoomIn"
        | "zoomOut"
        | "zoomReset";
    }
  | {
      type: "terminalCommand";
      panelId: string;
      command:
        | "sendCtrlF"
        | "clearScreenKeepScrollback"
        | "find"
        | "findNext"
        | "findPrevious"
        | "hideFind"
        | "useSelectionForFind"
        | "toggleTextBoxInput"
        | "focusTextBoxInput"
        | "attachTextBoxFile";
    }
  | { type: "openBrowser"; panelId: string }
  | {
      type: "forkAgentNewWorkspace";
      initialTerminalInput: string;
      currentDirectory?: string;
    }
  | { type: "warmClaudeCode"; currentDirectory?: string }
  | {
      type: "forkAgentNewTab";
      panelId: string;
      initialTerminalInput: string;
    }
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
  /** Control-socket `surface:N` ref for the focused panel, if derivable. */
  activeSurfaceRef?: string;
  /** pane_id of the focused pane, when the active layout carries pane ids. */
  activePaneId?: string;
  /** Control-socket `pane:N` ref for the focused pane, if derivable. */
  activePaneRef?: string;
  /** Active app callback scheme for same-session deep links. */
  navigationScheme?: string;
  /**
   * workspace_id of the selected workspace, verbatim from the snapshot;
   * undefined when absent.
   */
  selectedWorkspaceId?: string;
  /** Canonical command-palette workspace display name for edit-prefill flows. */
  selectedWorkspaceTitle?: string;
  /** Current tab display name for edit-prefill flows. */
  activePanelTitle?: string;
  /** Whether the focused panel/tab is pinned. */
  activePanelIsPinned?: boolean;
  /** Whether the focused panel/tab is currently marked unread. */
  activePanelHasUnread?: boolean;
  /** Current custom description, or the empty string when absent. */
  selectedWorkspaceDescription?: string;
  /**
   * The selected workspace's pin flag, threaded by the caller as
   * `workspaces[selectedWorkspaceIndex]?.is_pinned === true`. The snapshot
   * encodes `is_pinned` as `Some(true)|None` (A7 golden-stability; see the
   * `?? false` idiom at snapshotProjection.ts:80), so absent/undefined MUST
   * read as unpinned.
   */
  selectedWorkspaceIsPinned?: boolean;
  /** Whether the selected workspace currently carries an unread marker. */
  selectedWorkspaceIsUnread?: boolean;
  /** Active workspace layout mode; `"canvas"` means the freeform canvas view. */
  selectedWorkspaceLayoutMode?: string;
  /** Group id for the selected workspace, when it belongs to a sidebar group. */
  selectedWorkspaceGroupId?: string;
  /** Whether the selected workspace's group is currently collapsed. */
  selectedWorkspaceGroupIsCollapsed?: boolean;
  /** Fork command for the focused panel's restorable agent snapshot. */
  activeForkCommand?: string;
  /** Working directory to use for a forked terminal. */
  activeForkWorkingDirectory?: string;
  /** Working directory to use when preparing workspace-scoped warm agents. */
  selectedWorkspaceCurrentDirectory?: string;
  /** Directional pane-focus targets derived from the active layout geometry. */
  adjacentPanelIds?: Partial<Record<PaneFocusDirection, string>>;
}

/** Whether a resolved plan has a real host/session execution path. */
export function hasExecutablePlan(plan: IntentPlan): boolean {
  return plan.type !== "unhandled";
}

function workspaceRef(index: number): string {
  return `workspace:${index + 1}`;
}

/** Decide what `kind` does given the current session shape. */
export function planIntent(
  kind: CommandIntentKind,
  ctx: IntentPlanContext,
): IntentPlan {
  const {
    selectedWorkspaceIndex,
    workspaceCount,
    activePanelId,
    activeSurfaceRef,
    activePaneId,
    activePaneRef,
    navigationScheme,
    selectedWorkspaceId,
    selectedWorkspaceTitle,
    activePanelTitle,
    activePanelIsPinned,
    activePanelHasUnread,
    selectedWorkspaceDescription,
    selectedWorkspaceIsPinned,
    selectedWorkspaceIsUnread,
    selectedWorkspaceLayoutMode,
    selectedWorkspaceGroupId,
    selectedWorkspaceGroupIsCollapsed,
    activeForkCommand,
    activeForkWorkingDirectory,
    selectedWorkspaceCurrentDirectory,
    adjacentPanelIds,
  } = ctx;
  const hasWorkspaces =
    workspaceCount > 0 &&
    selectedWorkspaceIndex >= 0 &&
    selectedWorkspaceIndex < workspaceCount;
  const forkInput = forkStartupInput(activeForkCommand);

  switch (kind) {
    case "newWorkspace":
    case "newTerminalTab":
      return { type: "newWorkspace" };
    case "newBrowserWorkspace":
      return { type: "newBrowserWorkspace" };
    case "reopenClosedBrowserTab":
      return { type: "reopenClosedBrowserTab" };
    case "installCLI":
      return { type: "installCLI" };
    case "uninstallCLI":
      return { type: "uninstallCLI" };
    case "makeDefaultTerminal":
      return { type: "makeDefaultTerminal" };
    case "newBrowserTab":
      return activePanelId !== undefined
        ? { type: "openBrowser", panelId: activePanelId }
        : { type: "newBrowserWorkspace" };
    case "openFolder":
      return { type: "openFolder" };
    case "reopenPreviousSession":
      return { type: "restorePreviousLaunch" };
    case "closeWorkspace":
    case "closeTab":
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
    case "jumpUnread":
      return hasWorkspaces ? { type: "jumpUnread" } : { type: "none" };
    case "markOldestUnreadAndJumpNext":
      return hasWorkspaces ? { type: "markOldestUnreadAndJumpNext" } : { type: "none" };
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
    case "warmClaudeCode":
      return hasWorkspaces
        ? {
            type: "warmClaudeCode",
            currentDirectory: selectedWorkspaceCurrentDirectory,
          }
        : { type: "none" };
    case "forkAgentConversationRight":
      return activePanelId !== undefined && forkInput !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "horizontal",
            insertFirst: false,
            initialTerminalInput: forkInput,
            currentDirectory: activeForkWorkingDirectory,
          }
        : { type: "none" };
    case "forkAgentConversationLeft":
      return activePanelId !== undefined && forkInput !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "horizontal",
            insertFirst: true,
            initialTerminalInput: forkInput,
            currentDirectory: activeForkWorkingDirectory,
          }
        : { type: "none" };
    case "forkAgentConversationBottom":
      return activePanelId !== undefined && forkInput !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "vertical",
            insertFirst: false,
            initialTerminalInput: forkInput,
            currentDirectory: activeForkWorkingDirectory,
          }
        : { type: "none" };
    case "forkAgentConversationTop":
      return activePanelId !== undefined && forkInput !== undefined
        ? {
            type: "split",
            panelId: activePanelId,
            orientation: "vertical",
            insertFirst: true,
            initialTerminalInput: forkInput,
            currentDirectory: activeForkWorkingDirectory,
          }
        : { type: "none" };
    case "forkAgentConversationNewTab":
      return activePanelId !== undefined && forkInput !== undefined
        ? {
            type: "forkAgentNewTab",
            panelId: activePanelId,
            initialTerminalInput: forkInput,
          }
        : { type: "none" };
    case "forkAgentConversationNewWorkspace":
      return forkInput !== undefined
        ? {
            type: "forkAgentNewWorkspace",
            initialTerminalInput: forkInput,
            currentDirectory: activeForkWorkingDirectory,
          }
        : { type: "none" };
    case "terminalSplitBrowserRight":
    case "browserSplitRight":
    case "browserDuplicateRight":
      return activePanelId !== undefined
        ? {
            type: "splitBrowser",
            panelId: activePanelId,
            orientation: "horizontal",
            insertFirst: false,
          }
        : { type: "none" };
    case "terminalSplitBrowserDown":
    case "browserSplitDown":
      return activePanelId !== undefined
        ? {
            type: "splitBrowser",
            panelId: activePanelId,
            orientation: "vertical",
            insertFirst: false,
          }
        : { type: "none" };
    case "openDiffViewer":
    case "openDirectoryDiffViewer":
      return activePanelId !== undefined
        ? {
            type: "setSurfaceKind",
            panelId: activePanelId,
            kind: "diff",
          }
        : { type: "none" };
    case "openWorkspacePullRequests":
      return hasWorkspaces ? { type: "openWorkspacePullRequests" } : { type: "none" };
    case "restartSocketListener":
      return { type: "restartControlSocketListener" };
    case "markdownZoomIn":
      return activePanelId !== undefined
        ? { type: "markdownZoomIn", panelId: activePanelId }
        : { type: "none" };
    case "markdownZoomOut":
      return activePanelId !== undefined
        ? { type: "markdownZoomOut", panelId: activePanelId }
        : { type: "none" };
    case "markdownZoomReset":
      return activePanelId !== undefined
        ? { type: "markdownZoomReset", panelId: activePanelId }
        : { type: "none" };
    case "browserBack":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "back" }
        : { type: "none" };
    case "browserForward":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "forward" }
        : { type: "none" };
    case "browserReload":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "reload" }
        : { type: "none" };
    case "browserOpenDefault":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "openDefault" }
        : { type: "none" };
    case "browserFocusAddressBar":
      return activePanelId !== undefined
        ? {
            type: "browserCommand",
            panelId: activePanelId,
            command: "focusAddressBar",
          }
        : { type: "none" };
    case "browserToggleOmnibar":
      return activePanelId !== undefined
        ? { type: "toggleBrowserOmnibar", panelId: activePanelId }
        : { type: "none" };
    case "browserFocusMode":
      return activePanelId !== undefined
        ? { type: "toggleBrowserFocusMode", panelId: activePanelId }
        : { type: "none" };
    case "browserToggleDevTools":
      return activePanelId !== undefined
        ? { type: "toggleBrowserDeveloperTools", panelId: activePanelId }
        : { type: "none" };
    case "browserConsole":
      return activePanelId !== undefined
        ? {
            type: "showBrowserDeveloperTools",
            panelId: activePanelId,
            panel: "console",
          }
        : { type: "none" };
    case "browserReactGrab":
      return activePanelId !== undefined
        ? {
            type: "showBrowserDeveloperTools",
            panelId: activePanelId,
            panel: "react",
          }
        : { type: "none" };
    case "browserNetwork":
      return activePanelId !== undefined
        ? {
            type: "showBrowserDeveloperTools",
            panelId: activePanelId,
            panel: "network",
          }
        : { type: "none" };
    case "browserNetworkClear":
      return activePanelId !== undefined
        ? { type: "clearBrowserNetworkRecords", panelId: activePanelId }
        : { type: "none" };
    case "browserClearHistory":
      return activePanelId !== undefined
        ? { type: "clearBrowserHistory", panelId: activePanelId }
        : { type: "none" };
    case "browserZoomIn":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "zoomIn" }
        : { type: "none" };
    case "browserZoomOut":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "zoomOut" }
        : { type: "none" };
    case "browserZoomReset":
      return activePanelId !== undefined
        ? { type: "browserCommand", panelId: activePanelId, command: "zoomReset" }
        : { type: "none" };
    case "terminalSendCtrlF":
      return activePanelId !== undefined
        ? { type: "terminalCommand", panelId: activePanelId, command: "sendCtrlF" }
        : { type: "none" };
    case "terminalFind":
      return activePanelId !== undefined
        ? { type: "terminalCommand", panelId: activePanelId, command: "find" }
        : { type: "none" };
    case "terminalFindNext":
      return activePanelId !== undefined
        ? { type: "terminalCommand", panelId: activePanelId, command: "findNext" }
        : { type: "none" };
    case "terminalFindPrevious":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "findPrevious",
          }
        : { type: "none" };
    case "terminalHideFind":
      return activePanelId !== undefined
        ? { type: "terminalCommand", panelId: activePanelId, command: "hideFind" }
        : { type: "none" };
    case "terminalUseSelectionForFind":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "useSelectionForFind",
          }
        : { type: "none" };
    case "terminalToggleTextBoxInput":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "toggleTextBoxInput",
          }
        : { type: "none" };
    case "terminalFocusTextBoxInput":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "focusTextBoxInput",
          }
        : { type: "none" };
    case "terminalAttachTextBoxFile":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "attachTextBoxFile",
          }
        : { type: "none" };
    case "terminalClearScreenKeepScrollback":
      return activePanelId !== undefined
        ? {
            type: "terminalCommand",
            panelId: activePanelId,
            command: "clearScreenKeepScrollback",
          }
        : { type: "none" };
    case "copyWorkspaceID":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? { type: "copyText", text: `workspace_id=${selectedWorkspaceId}` }
        : { type: "none" };
    case "copyWorkspaceIDAndRef":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? {
            type: "copyText",
            text: [
              `workspace_id=${selectedWorkspaceId}`,
              `workspace_ref=${workspaceRef(selectedWorkspaceIndex)}`,
            ].join("\n"),
          }
        : { type: "none" };
    case "copyWorkspaceLink":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? {
            type: "copyText",
            text: workspaceLink(selectedWorkspaceId, navigationScheme),
          }
        : { type: "none" };
    case "copyPaneID":
      return activePaneId !== undefined
        ? { type: "copyText", text: `pane_id=${activePaneId}` }
        : { type: "none" };
    case "copyPaneLink":
      return hasWorkspaces &&
        selectedWorkspaceId !== undefined &&
        activePaneId !== undefined
        ? {
            type: "copyText",
            text: paneLink(selectedWorkspaceId, activePaneId, navigationScheme),
          }
        : { type: "none" };
    case "copySurfaceID":
      return activePanelId !== undefined
        ? { type: "copyText", text: `surface_id=${activePanelId}` }
        : { type: "none" };
    case "copySurfaceLink":
      return hasWorkspaces &&
        selectedWorkspaceId !== undefined &&
        activePanelId !== undefined
        ? {
            type: "copyText",
            text: surfaceLink(selectedWorkspaceId, activePanelId, navigationScheme),
          }
        : { type: "none" };
    case "copyIdentifiers":
      return hasWorkspaces &&
        selectedWorkspaceId !== undefined &&
        activePanelId !== undefined
        ? {
            type: "copyText",
            text: [
              `workspace_id=${selectedWorkspaceId}`,
              `workspace_ref=${workspaceRef(selectedWorkspaceIndex)}`,
              ...(activePaneId !== undefined ? [`pane_id=${activePaneId}`] : []),
              ...(activePaneRef !== undefined ? [`pane_ref=${activePaneRef}`] : []),
              `surface_id=${activePanelId}`,
              ...(activeSurfaceRef !== undefined
                ? [`surface_ref=${activeSurfaceRef}`]
                : []),
            ].join("\n"),
          }
        : { type: "none" };
    case "equalizeSplits":
      return { type: "equalizeDividers" };
    case "toggleWorkspacePin":
      return hasWorkspaces
        ? {
            type: "setWorkspacePinned",
            index: selectedWorkspaceIndex,
            pinned: !(selectedWorkspaceIsPinned === true),
          }
        : { type: "none" };
    case "markWorkspaceRead":
      return hasWorkspaces
        ? {
            type: "setWorkspaceUnread",
            index: selectedWorkspaceIndex,
            unread: false,
            preferredPanelId: activePanelId,
          }
        : { type: "none" };
    case "markWorkspaceUnread":
      return hasWorkspaces
        ? {
            type: "setWorkspaceUnread",
            index: selectedWorkspaceIndex,
            unread: true,
            preferredPanelId: activePanelId,
          }
        : { type: "none" };
    case "toggleUnread":
      return hasWorkspaces
        ? {
            type: "setWorkspaceUnread",
            index: selectedWorkspaceIndex,
            unread: !(selectedWorkspaceIsUnread === true),
            preferredPanelId: activePanelId,
          }
        : { type: "none" };
    case "resetWorkspaceColor":
      return hasWorkspaces
        ? { type: "resetWorkspaceColor", index: selectedWorkspaceIndex }
        : { type: "none" };
    case "closeOtherWorkspaces":
      return hasWorkspaces && workspaceCount > 1
        ? {
            type: "closeWorkspaces",
            indexes: Array.from({ length: workspaceCount }, (_, index) => index).filter(
              (index) => index !== selectedWorkspaceIndex,
            ),
          }
        : { type: "none" };
    case "closeWorkspacesAbove":
      return hasWorkspaces && selectedWorkspaceIndex > 0
        ? {
            type: "closeWorkspaces",
            indexes: Array.from({ length: selectedWorkspaceIndex }, (_, index) => index),
          }
        : { type: "none" };
    case "closeWorkspacesBelow":
      return hasWorkspaces && selectedWorkspaceIndex < workspaceCount - 1
        ? {
            type: "closeWorkspaces",
            indexes: Array.from(
              { length: workspaceCount - selectedWorkspaceIndex - 1 },
              (_, offset) => selectedWorkspaceIndex + 1 + offset,
            ),
          }
        : { type: "none" };
    case "collapseWorkspaceGroup":
      return hasWorkspaces &&
        selectedWorkspaceGroupId !== undefined &&
        selectedWorkspaceGroupIsCollapsed !== true
        ? {
            type: "setWorkspaceGroupCollapsed",
            groupId: selectedWorkspaceGroupId,
            collapsed: true,
          }
        : { type: "none" };
    case "expandWorkspaceGroup":
      return hasWorkspaces &&
        selectedWorkspaceGroupId !== undefined &&
        selectedWorkspaceGroupIsCollapsed === true
        ? {
            type: "setWorkspaceGroupCollapsed",
            groupId: selectedWorkspaceGroupId,
            collapsed: false,
          }
        : { type: "none" };
    case "moveWorkspaceUp":
      return hasWorkspaces && selectedWorkspaceIndex > 0
        ? {
            type: "reorderWorkspace",
            index: selectedWorkspaceIndex,
            toIndex: selectedWorkspaceIndex - 1,
          }
        : { type: "none" };
    case "moveWorkspaceDown":
      return hasWorkspaces && selectedWorkspaceIndex < workspaceCount - 1
        ? {
            type: "reorderWorkspace",
            index: selectedWorkspaceIndex,
            toIndex: selectedWorkspaceIndex + 1,
          }
        : { type: "none" };
    case "moveWorkspaceToTop":
      return hasWorkspaces && selectedWorkspaceIndex > 0
        ? {
            type: "reorderWorkspace",
            index: selectedWorkspaceIndex,
            toIndex: 0,
          }
        : { type: "none" };
    case "renameWorkspace":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? {
            type: "beginRenameWorkspace",
            workspaceId: selectedWorkspaceId,
            title: selectedWorkspaceTitle ?? "Workspace",
          }
        : { type: "none" };
    case "editWorkspaceDescription":
      return hasWorkspaces && selectedWorkspaceId !== undefined
        ? {
            type: "beginWorkspaceDescriptionEdit",
            workspaceId: selectedWorkspaceId,
            title: selectedWorkspaceTitle ?? "Workspace",
            description: selectedWorkspaceDescription ?? "",
          }
        : { type: "none" };
    case "clearWorkspaceName":
      return hasWorkspaces
        ? { type: "renameWorkspace", index: selectedWorkspaceIndex, title: "" }
        : { type: "none" };
    case "clearWorkspaceDescription":
      return hasWorkspaces
        ? {
            type: "setWorkspaceDescription",
            index: selectedWorkspaceIndex,
            description: "",
          }
        : { type: "none" };
    case "renameTab":
      return activePanelId !== undefined
        ? {
            type: "beginRenameTab",
            panelId: activePanelId,
            title: activePanelTitle ?? "Tab",
          }
        : { type: "none" };
    case "clearTabName":
      return activePanelId !== undefined
        ? { type: "renameTab", panelId: activePanelId, title: "" }
        : { type: "none" };
    case "toggleTabPin":
      return activePanelId !== undefined
        ? {
            type: "setPanelPinned",
            panelId: activePanelId,
            pinned: !(activePanelIsPinned === true),
          }
        : { type: "none" };
    case "toggleTabUnread":
      return activePanelId !== undefined
        ? {
            type: "setPanelUnread",
            panelId: activePanelId,
            unread: !(activePanelHasUnread === true),
          }
        : { type: "none" };
    case "moveTabToNewWorkspace":
      return activePanelId !== undefined
        ? { type: "movePanelToNewWorkspace", panelId: activePanelId }
        : { type: "none" };
    case "nextTabInPane":
      return activePanelId !== undefined
        ? { type: "selectAdjacentPanel", panelId: activePanelId, next: true }
        : { type: "none" };
    case "previousTabInPane":
      return activePanelId !== undefined
        ? { type: "selectAdjacentPanel", panelId: activePanelId, next: false }
        : { type: "none" };
    case "focusLeft":
      return adjacentPanelIds?.left !== undefined
        ? { type: "focusPanel", panelId: adjacentPanelIds.left }
        : { type: "none" };
    case "focusRight":
      return adjacentPanelIds?.right !== undefined
        ? { type: "focusPanel", panelId: adjacentPanelIds.right }
        : { type: "none" };
    case "focusUp":
      return adjacentPanelIds?.up !== undefined
        ? { type: "focusPanel", panelId: adjacentPanelIds.up }
        : { type: "none" };
    case "focusDown":
      return adjacentPanelIds?.down !== undefined
        ? { type: "focusPanel", panelId: adjacentPanelIds.down }
        : { type: "none" };
    case "toggleSplitZoom":
      return activePanelId !== undefined
        ? { type: "toggleSplitZoom", panelId: activePanelId }
        : { type: "none" };
    case "toggleCanvasLayout":
      return hasWorkspaces
        ? {
            type: "setLayoutMode",
            mode: selectedWorkspaceLayoutMode === "canvas" ? null : "canvas",
          }
        : { type: "none" };
    case "triggerFlash":
      return activePanelId !== undefined
        ? { type: "triggerPanelFlash", panelId: activePanelId }
        : { type: "none" };
    case "newWindow":
      return { type: "newWindow" };
    case "closeWindow":
      return { type: "closeWindow" };
    case "toggleFullScreen":
      return { type: "toggleFullScreen" };
    case "openTaskManager":
      return { type: "openTaskManager" };
    case "toggleSidebar":
      return { type: "toggleSidebar" };
    case "toggleFileExplorer":
      return hasWorkspaces ? { type: "toggleFileExplorer" } : { type: "none" };
    case "toggleMatchTerminalBackground":
      return { type: "toggleMatchTerminalBackground" };
    case "enableMinimalMode":
      return { type: "setMinimalMode", enabled: true };
    case "disableMinimalMode":
      return { type: "setMinimalMode", enabled: false };
    case "disableBrowser":
      return { type: "setBrowserEnabled", enabled: false };
    case "enableBrowser":
      return { type: "setBrowserEnabled", enabled: true };
    case "openSettings":
      return { type: "openSettings" };
    case "mobileConnect":
      return { type: "openSettings", section: "mobile" };
    case "authSignIn":
    case "authSignOut":
      return { type: "openSettings", section: "account" };
    case "checkForUpdates":
    case "attemptUpdate":
    case "applyUpdateIfAvailable":
      return { type: "openSettings", section: "updates" };
    case "showNotifications":
      return { type: "showNotifications" };
    case "findInDirectory":
      return hasWorkspaces ? { type: "openFindInDirectory" } : { type: "none" };
    case "openFolderInVSCodeInline":
      return { type: "openFolderInVSCodeInline" };
    case "vscodeServeWebStop":
      return { type: "vscodeServeWebStop" };
    case "vscodeServeWebRestart":
      return { type: "vscodeServeWebRestart" };
    case "openCmuxSettingsFile":
      return { type: "openCmuxSettingsFile" };
    case "openGhosttySettings":
      return { type: "openGhosttySettingsFile" };
    default:
      return { type: "unhandled" };
  }
}

function forkStartupInput(command: string | undefined): string | undefined {
  const trimmed = command?.trim();
  if (trimmed == null || trimmed === "") {
    return undefined;
  }
  return `${trimmed}\r\n`;
}
