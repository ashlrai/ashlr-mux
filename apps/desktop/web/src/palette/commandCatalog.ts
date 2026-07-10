// Command-palette command catalog + one shared dispatch path. Port of the
// macOS host's declarative command contribution list and its build loop:
//
//   - Descriptor value type       `CommandPaletteCommand.swift:5-52`
//   - Contribution descriptor      `CommandPaletteCommandContribution.swift:6-44`
//   - Build loop (filter + rank)   `Sources/ContentView.swift:6010-6052`
//   - Config-override + `.palette`  `Sources/ContentView.swift:6023-6043`, 6054-6067
//   - The contribution rows         `Sources/ContentView.swift:6321-7464`
//                                   + `ContentView+ViewCommandPalette.swift:5-24`
//                                   + `ContentViewIdentifierCopyCommands.swift:7-88`
//                                   + `ContentView+MoveTabToNewWorkspace.swift:8-19`
//                                   + `ContentView+AuthCommandPalette.swift:9-35`
//   - One shared run path           `Sources/ContentView.swift:8499-8523`
//     (`runCommandPaletteCommand`), handler registry indirection
//     `CommandPaletteHandlerRegistry.swift:6-21`.
//
// This module is PURE and headless (no React / Tauri / DOM), matching sibling
// `paletteQuery.ts` / `listScope.ts`. Only value-in / value-out.
//
// LOCALIZATION (flagged to the orchestrator): the Swift rows use
// `String(localized:defaultValue:)`. Per CLAUDE.md localization policy the
// English `defaultValue` is the source string but real localization goes
// through `web/messages/en.json` / `ja.json`. For this headless pure module we
// inline the English defaults with `// LOCALIZE` markers; D4/D10 wire a message
// lookup. Do NOT ship these as final user strings without the catalog wiring.
//
// HOST/RUNTIME-DRIVEN SUB-LISTS (the extension-sidebar loop 6473, right-sidebar
// mode/tool-pane 6485-6486, canvas 6518, settings-toggle 6672, workspace-color
// palette 6738, terminal open targets 7156, and cmux.json config issues/custom
// actions 7438-7462) derive their titles/keywords/gates from live host state
// that is not available headless. They are modeled as injectable insertion
// points (see `InsertionPoint` / `buildCommandCatalog` `dynamicContributions`)
// spliced at the exact Swift `contentsOf:` positions so absolute-rank parity is
// reachable once D4 supplies the runtime rows. The default catalog omits them.

import type { ConfigAction } from "../settings/configReducer";
import type { RightSidebarMode } from "../rightSidebarModes";

/**
 * The `when` / `enablement` gate inputs referenced by the ported contribution
 * rows — a port of the `CommandPaletteContextKeys` booleans the host evaluates
 * against its `CommandPaletteContextSnapshot`
 * (`Sources/ContentView.swift:6337, 6354, 6363, 6417, 6491, 6505, 6514, …`).
 *
 * Modeled as an explicit typed record (not a `[key: string]: boolean` bag) so
 * the test oracle is exhaustive. Every boolean defaults to `false` and every
 * string defaults to absent, mirroring Swift `snapshot.bool(_)` / `.string(_)`.
 */
export interface CommandContext {
  // Global / availability
  browserDisabled?: boolean;
  cliInstalledInPATH?: boolean;
  /** Host `TerminalDirectoryOpenTarget.vscodeInline.isAvailable()` (6390) and
   *  `terminalOpenTargetAvailable(.vscodeInline)` (7177), surfaced as a bool. */
  vscodeInlineOpenTargetAvailable?: boolean;
  defaultTerminalIsDefault?: boolean;
  updateHasAvailable?: boolean;
  authSignedIn?: boolean;
  authWorking?: boolean;

  // Layout / sidebar
  workspaceMinimalModeEnabled?: boolean;
  sidebarMatchTerminalBackground?: boolean;
  workspaceCanvasLayout?: boolean;

  // Workspace scope
  hasWorkspace?: boolean;
  workspaceHasCustomName?: boolean;
  workspaceHasCustomDescription?: boolean;
  workspaceShouldPin?: boolean;
  workspaceHasAbove?: boolean;
  workspaceHasBelow?: boolean;
  workspaceHasPeers?: boolean;
  workspaceCanMoveUp?: boolean;
  workspaceCanMoveDown?: boolean;
  workspaceCanMarkRead?: boolean;
  workspaceCanMarkUnread?: boolean;
  workspaceGroupCanCollapse?: boolean;
  workspaceGroupCanExpand?: boolean;
  workspaceHasPullRequests?: boolean;
  workspaceHasSplits?: boolean;

  // Panel / surface scope
  hasFocusedPanel?: boolean;
  panelHasCustomName?: boolean;
  panelShouldPin?: boolean;
  panelHasUnread?: boolean;
  panelHasPane?: boolean;
  panelCanMoveToNewWorkspace?: boolean;
  panelIsBrowser?: boolean;
  panelIsMarkdown?: boolean;
  panelIsTerminal?: boolean;
  panelBrowserFocusModeActive?: boolean;
  panelBrowserOmnibarVisible?: boolean;
  panelHasForkableAgent?: boolean;

  // Subtitle name interpolation (Swift `context.string(workspaceName/panelName)`)
  workspaceName?: string | null;
  panelName?: string | null;
}

/**
 * A cmux.json config-driven palette-action override, the headless projection of
 * `cmuxConfigStore.resolvedAction(id:)` (`Sources/ContentView.swift:6024`).
 * When `palette` is false the row is dropped BEFORE rank assignment
 * (6025-6027). Non-empty `title` / `subtitle` / `keywords` override the
 * contribution's own (6037-6043).
 */
export interface ConfigPaletteAction {
  palette: boolean;
  title?: string;
  subtitle?: string;
  keywords?: string[];
}

/**
 * Injectable config-override hook. Default resolver returns `null` (identity:
 * all rows keep `palette: true` with no overrides), keeping this module
 * headless; D4/E-area config wiring supplies the real resolver.
 *
 * Only the four ids in {@link COMMAND_PALETTE_CONFIGURABLE_COMMAND_IDS} are
 * config-overridable in the host (`commandPaletteConfigActionID`, 6054-6067) —
 * a faithful resolver returns `null` for every other id.
 */
export type ConfigResolver = (commandId: string) => ConfigPaletteAction | null;

/**
 * The command ids whose title/subtitle/keywords/`.palette` gate the host reads
 * from cmux.json. Port of the `commandPaletteConfigActionID` switch
 * (`Sources/ContentView.swift:6054-6067`); the concrete cmux.json action ids
 * (`CmuxSurfaceTabBarBuiltInAction.*.configID`) are host-owned and resolved by
 * D4, so only the gated command-id set is exposed here.
 */
export const COMMAND_PALETTE_CONFIGURABLE_COMMAND_IDS: readonly string[] = [
  "palette.newTerminalTab",
  "palette.newBrowserTab",
  "palette.terminalSplitRight",
  "palette.terminalSplitDown",
];

// ---------------------------------------------------------------------------
// Intent registry — the single id → behavior mapping.
// Port of `CommandPaletteHandlerRegistry` (handler registry indirection): each
// command id maps to exactly ONE runnable behavior. Here that behavior is a
// discriminated `CommandIntent`; the React/Tauri invocation lands in D4. Keep
// this the ONLY place an id is mapped to what it does (shared-behavior rule).
// ---------------------------------------------------------------------------

/**
 * The single source-of-truth id → intent-kind table. Every statically-cataloged
 * command id appears exactly once; the intent kind is a stable behavior key.
 * `CommandIntentKind` and {@link resolveIntent} are both derived from this.
 */
const INTENT_KIND_BY_COMMAND_ID = {
  "palette.newWorkspace": "newWorkspace",
  "palette.newBrowserWorkspace": "newBrowserWorkspace",
  "palette.newWindow": "newWindow",
  "palette.installCLI": "installCLI",
  "palette.uninstallCLI": "uninstallCLI",
  "palette.openFolder": "openFolder",
  "palette.openFolderInVSCodeInline": "openFolderInVSCodeInline",
  "palette.reopenPreviousSession": "reopenPreviousSession",
  "palette.newTerminalTab": "newTerminalTab",
  "palette.newBrowserTab": "newBrowserTab",
  "palette.closeTab": "closeTab",
  "palette.closeWorkspace": "closeWorkspace",
  "palette.closeWindow": "closeWindow",
  "palette.toggleFullScreen": "toggleFullScreen",
  "palette.reopenClosedBrowserTab": "reopenClosedBrowserTab",
  "palette.toggleSidebar": "toggleSidebar",
  "palette.toggleFileExplorer": "toggleFileExplorer",
  "palette.toggleMatchTerminalBackground": "toggleMatchTerminalBackground",
  "palette.enableMinimalMode": "enableMinimalMode",
  "palette.disableMinimalMode": "disableMinimalMode",
  "palette.triggerFlash": "triggerFlash",
  "palette.openTaskManager": "openTaskManager",
  "palette.canvas.toggleLayout": "toggleCanvasLayout",
  "palette.showNotifications": "showNotifications",
  "palette.jumpUnread": "jumpUnread",
  "palette.toggleUnread": "toggleUnread",
  "palette.markOldestUnreadAndJumpNext": "markOldestUnreadAndJumpNext",
  "palette.openSettings": "openSettings",
  "palette.openCmuxSettingsFile": "openCmuxSettingsFile",
  "palette.openGhosttySettings": "openGhosttySettings",
  "palette.mobileConnect": "mobileConnect",
  "palette.auth.signIn": "authSignIn",
  "palette.auth.signOut": "authSignOut",
  "palette.makeDefaultTerminal": "makeDefaultTerminal",
  "palette.checkForUpdates": "checkForUpdates",
  "palette.applyUpdateIfAvailable": "applyUpdateIfAvailable",
  "palette.attemptUpdate": "attemptUpdate",
  "palette.restartSocketListener": "restartSocketListener",
  "palette.disableBrowser": "disableBrowser",
  "palette.enableBrowser": "enableBrowser",
  "palette.renameWorkspace": "renameWorkspace",
  "palette.editWorkspaceDescription": "editWorkspaceDescription",
  "palette.clearWorkspaceName": "clearWorkspaceName",
  "palette.clearWorkspaceDescription": "clearWorkspaceDescription",
  "palette.toggleWorkspacePin": "toggleWorkspacePin",
  "palette.resetWorkspaceColor": "resetWorkspaceColor",
  "palette.nextWorkspace": "nextWorkspace",
  "palette.previousWorkspace": "previousWorkspace",
  "palette.moveWorkspaceUp": "moveWorkspaceUp",
  "palette.moveWorkspaceDown": "moveWorkspaceDown",
  "palette.moveWorkspaceToTop": "moveWorkspaceToTop",
  "palette.closeOtherWorkspaces": "closeOtherWorkspaces",
  "palette.closeWorkspacesBelow": "closeWorkspacesBelow",
  "palette.closeWorkspacesAbove": "closeWorkspacesAbove",
  "palette.collapseWorkspaceGroup": "collapseWorkspaceGroup",
  "palette.expandWorkspaceGroup": "expandWorkspaceGroup",
  "palette.markWorkspaceRead": "markWorkspaceRead",
  "palette.markWorkspaceUnread": "markWorkspaceUnread",
  "palette.copyWorkspaceID": "copyWorkspaceID",
  "palette.copyWorkspaceIDAndRef": "copyWorkspaceIDAndRef",
  "palette.copyWorkspaceLink": "copyWorkspaceLink",
  "palette.copyPaneID": "copyPaneID",
  "palette.copyPaneLink": "copyPaneLink",
  "palette.copySurfaceID": "copySurfaceID",
  "palette.copySurfaceLink": "copySurfaceLink",
  "palette.copyIdentifiers": "copyIdentifiers",
  "palette.renameTab": "renameTab",
  "palette.clearTabName": "clearTabName",
  "palette.moveTabToNewWorkspace": "moveTabToNewWorkspace",
  "palette.toggleTabPin": "toggleTabPin",
  "palette.toggleTabUnread": "toggleTabUnread",
  "palette.nextTabInPane": "nextTabInPane",
  "palette.previousTabInPane": "previousTabInPane",
  "palette.openWorkspacePullRequests": "openWorkspacePullRequests",
  "palette.openDiffViewer": "openDiffViewer",
  "palette.openDirectoryDiffViewer": "openDirectoryDiffViewer",
  "palette.browserBack": "browserBack",
  "palette.browserForward": "browserForward",
  "palette.browserReload": "browserReload",
  "palette.browserOpenDefault": "browserOpenDefault",
  "palette.browserFocusAddressBar": "browserFocusAddressBar",
  "palette.browserFocusMode": "browserFocusMode",
  "palette.browserToggleOmnibar": "browserToggleOmnibar",
  "palette.browserToggleDevTools": "browserToggleDevTools",
  "palette.browserConsole": "browserConsole",
  "palette.browserReactGrab": "browserReactGrab",
  "palette.browserNetwork": "browserNetwork",
  "palette.browserNetworkClear": "browserNetworkClear",
  "palette.browserZoomIn": "browserZoomIn",
  "palette.browserZoomOut": "browserZoomOut",
  "palette.browserZoomReset": "browserZoomReset",
  "palette.markdownZoomIn": "markdownZoomIn",
  "palette.markdownZoomOut": "markdownZoomOut",
  "palette.markdownZoomReset": "markdownZoomReset",
  "palette.browserClearHistory": "browserClearHistory",
  "palette.browserSplitRight": "browserSplitRight",
  "palette.browserSplitDown": "browserSplitDown",
  "palette.browserDuplicateRight": "browserDuplicateRight",
  "palette.vscodeServeWebStop": "vscodeServeWebStop",
  "palette.vscodeServeWebRestart": "vscodeServeWebRestart",
  "palette.findInDirectory": "findInDirectory",
  "palette.terminalFind": "terminalFind",
  "palette.terminalFindNext": "terminalFindNext",
  "palette.terminalFindPrevious": "terminalFindPrevious",
  "palette.terminalHideFind": "terminalHideFind",
  "palette.terminalUseSelectionForFind": "terminalUseSelectionForFind",
  "palette.terminalToggleTextBoxInput": "terminalToggleTextBoxInput",
  "palette.terminalFocusTextBoxInput": "terminalFocusTextBoxInput",
  "palette.terminalAttachTextBoxFile": "terminalAttachTextBoxFile",
  "palette.terminalSendCtrlF": "terminalSendCtrlF",
  "palette.terminalClearScreenKeepScrollback": "terminalClearScreenKeepScrollback",
  "palette.focusLeft": "focusLeft",
  "palette.focusRight": "focusRight",
  "palette.focusUp": "focusUp",
  "palette.focusDown": "focusDown",
  "palette.terminalSplitRight": "terminalSplitRight",
  "palette.warmClaudeCode": "warmClaudeCode",
  "palette.forkAgentConversationRight": "forkAgentConversationRight",
  "palette.forkAgentConversationLeft": "forkAgentConversationLeft",
  "palette.forkAgentConversationTop": "forkAgentConversationTop",
  "palette.forkAgentConversationBottom": "forkAgentConversationBottom",
  "palette.forkAgentConversationNewTab": "forkAgentConversationNewTab",
  "palette.forkAgentConversationNewWorkspace": "forkAgentConversationNewWorkspace",
  "palette.terminalSplitDown": "terminalSplitDown",
  "palette.terminalSplitBrowserRight": "terminalSplitBrowserRight",
  "palette.terminalSplitBrowserDown": "terminalSplitBrowserDown",
  "palette.toggleSplitZoom": "toggleSplitZoom",
  "palette.equalizeSplits": "equalizeSplits",
} as const;

/** One distinct handler behavior; a stable key decoupled from display strings. */
export type CommandIntentKind =
  (typeof INTENT_KIND_BY_COMMAND_ID)[keyof typeof INTENT_KIND_BY_COMMAND_ID];

export type CanvasCommandAction =
  | "tidy"
  | "alignLeft"
  | "alignRight"
  | "alignTop"
  | "alignBottom"
  | "equalizeWidths"
  | "equalizeHeights"
  | "distributeHorizontally"
  | "distributeVertically";

/**
 * What activating a command does. Static rows resolve to a stable `kind`;
 * runtime-injected rows may carry extra payload (for example a concrete
 * settings toggle action). The host maps each case to its side effect; the pure
 * catalog never runs one.
 */
export type CommandIntent =
  | { kind: CommandIntentKind }
  | { kind: "toggleSetting"; action: ConfigAction }
  | { kind: "canvasAction"; action: CanvasCommandAction }
  | { kind: "rightSidebarMode"; mode: RightSidebarMode };

/**
 * The single id → intent mapping (parity of the handler registry). Returns
 * `null` for an unknown id — the caller then drops the row, mirroring the
 * `assertionFailure` + `continue` when no handler is registered
 * (`Sources/ContentView.swift:6029-6032`).
 */
export function resolveIntent(commandId: string): CommandIntent | null {
  const kind = (INTENT_KIND_BY_COMMAND_ID as Record<string, CommandIntentKind>)[
    commandId
  ];
  return kind === undefined ? null : { kind };
}

// ---------------------------------------------------------------------------
// Descriptor + contribution value types
// ---------------------------------------------------------------------------

/**
 * The static, headless subset of Swift `CommandPaletteCommand` (5-52): the
 * `() -> Void` action is dropped and replaced by the resolved {@link intent}.
 */
export interface CommandDescriptor {
  id: string;
  /** Post-filter sequential index; tie-break, lower wins (6020/6048). */
  rank: number;
  title: string;
  subtitle: string;
  shortcutHint?: string;
  /** Always `null` for command-scope rows (6040); surface rows carry a kind. */
  kindLabel: string | null;
  keywords: string[];
  dismissOnRun: boolean;
  /** The resolved behavior (id → intent), the pure analog of the action. */
  intent: CommandIntent;
}

/**
 * Texts the search corpus indexes for a command. Mirrors
 * `CommandPaletteCommand.searchableTexts` (`CommandPaletteCommand.swift:49-51`):
 * `[title, subtitle] + keywords`.
 */
export function searchableTexts(descriptor: {
  title: string;
  subtitle: string;
  keywords: string[];
}): string[] {
  return [descriptor.title, descriptor.subtitle, ...descriptor.keywords];
}

/**
 * Declarative contribution row — port of `CommandPaletteCommandContribution`
 * (6-44). `title`/`subtitle` are context-derived (Swift closures) so
 * toggle-titles and name-interpolated subtitles port faithfully.
 */
export interface CommandContribution {
  commandId: string;
  title: (ctx: CommandContext) => string;
  subtitle: (ctx: CommandContext) => string;
  shortcutHint?: string;
  keywords: string[];
  dismissOnRun: boolean;
  when: (ctx: CommandContext) => boolean;
  enablement: (ctx: CommandContext) => boolean;
  /** Optional explicit intent for injected dynamic rows; static rows resolve
   *  via {@link resolveIntent}. */
  intent?: CommandIntent;
}

/** Insertion points for host/runtime-driven sub-lists (see file header). */
export type InsertionPoint =
  | "extensionSidebar" // 6473 loop over CmuxExtensionSidebarSelection.descriptors
  | "rightSidebarMode" // 6485 commandPaletteRightSidebarModeCommandContributions
  | "rightSidebarToolPane" // 6486
  | "canvas" // 6518 commandPaletteCanvasCommandContributions (titles = Action.label)
  | "settingsToggle" // 6672 commandPaletteSettingsToggleCommandContributions
  | "workspaceColors" // 6738 loop over WorkspaceTabColorSettings.palette()
  | "terminalOpenTargets" // 7156 loop over TerminalDirectoryOpenTarget targets
  | "configIssues" // 7438 loop over cmuxConfigStore.configurationIssues
  | "configCustomActions"; // 7448 loop over cmuxConfigStore.paletteCustomActions()

/** Runtime sub-lists supplied by D4, spliced at their declared Swift position. */
export type DynamicContributions = Partial<
  Record<InsertionPoint, CommandContribution[]>
>;

export interface BuildCommandCatalogOptions {
  configResolver?: ConfigResolver;
  dynamicContributions?: DynamicContributions;
}

// ---------------------------------------------------------------------------
// Contribution builders (port of the row literals)
// ---------------------------------------------------------------------------

/** A context-independent string, mirroring the Swift `constant(_:)` helper. */
function constant(value: string): (ctx: CommandContext) => string {
  return () => value;
}

const alwaysTrue = (): boolean => true;

/** Fills contribution defaults, mirroring the Swift init defaults (24-33). */
function row(spec: {
  commandId: string;
  title: (ctx: CommandContext) => string;
  subtitle: (ctx: CommandContext) => string;
  shortcutHint?: string;
  keywords?: string[];
  dismissOnRun?: boolean;
  when?: (ctx: CommandContext) => boolean;
  enablement?: (ctx: CommandContext) => boolean;
}): CommandContribution {
  return {
    commandId: spec.commandId,
    title: spec.title,
    subtitle: spec.subtitle,
    shortcutHint: spec.shortcutHint,
    keywords: spec.keywords ?? [],
    dismissOnRun: spec.dismissOnRun ?? true,
    when: spec.when ?? alwaysTrue,
    enablement: spec.enablement ?? alwaysTrue,
  };
}

// Subtitle builders — port of the local funcs at `ContentView.swift:6268-6291`.
// LOCALIZE: default templates below.
function workspaceSubtitle(ctx: CommandContext): string {
  const name = ctx.workspaceName ?? "Workspace";
  return `Workspace • ${name}`;
}
function panelSubtitle(ctx: CommandContext): string {
  const name = ctx.panelName ?? "Tab";
  return `Tab • ${name}`;
}
function browserPanelSubtitle(ctx: CommandContext): string {
  const name = ctx.panelName ?? "Tab";
  return `Browser • ${name}`;
}
function terminalPanelSubtitle(ctx: CommandContext): string {
  const name = ctx.panelName ?? "Tab";
  return `Terminal • ${name}`;
}
function markdownPanelSubtitle(ctx: CommandContext): string {
  const name = ctx.panelName ?? "Tab";
  return `Markdown • ${name}`;
}

/**
 * The static contribution rows in EXACT declared order
 * (`Sources/ContentView.swift:6323-7154` static rows, plus the four static
 * sub-lists), interleaved with insertion-point markers for the runtime
 * sub-lists. Order is load-bearing: it drives the post-filter `rank`
 * tie-break, so do not reorder. All display strings are English defaults with
 * `// LOCALIZE` intent (see file header).
 */
function buildContributions(dynamic: DynamicContributions): CommandContribution[] {
  const out: CommandContribution[] = [];
  const push = (c: CommandContribution): void => {
    out.push(c);
  };
  const splice = (point: InsertionPoint): void => {
    for (const c of dynamic[point] ?? []) {
      out.push(c);
    }
  };

  // LOCALIZE (all inline strings in this function).
  push(
    row({
      commandId: "palette.newWorkspace",
      title: constant("New Workspace"),
      subtitle: constant("Workspace"),
      keywords: ["create", "new", "workspace"],
    }),
  );
  push(
    row({
      commandId: "palette.newBrowserWorkspace",
      title: constant("New Browser Workspace"),
      subtitle: constant("Workspace"),
      keywords: ["create", "new", "browser", "workspace", "web"],
      when: (c) => !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.newWindow",
      title: constant("New Window"),
      subtitle: constant("Window"),
      keywords: ["create", "new", "window"],
    }),
  );
  push(
    row({
      commandId: "palette.installCLI",
      title: constant("Shell Command: Install 'cmux' in PATH"),
      subtitle: constant("CLI"),
      keywords: ["install", "cli", "path", "shell", "command", "symlink"],
      when: (c) => !c.cliInstalledInPATH,
    }),
  );
  push(
    row({
      commandId: "palette.uninstallCLI",
      title: constant("Shell Command: Uninstall 'cmux' from PATH"),
      subtitle: constant("CLI"),
      keywords: ["uninstall", "remove", "cli", "path", "shell", "command", "symlink"],
      when: (c) => c.cliInstalledInPATH === true,
    }),
  );
  push(
    row({
      commandId: "palette.openFolder",
      title: constant("Open Folder…"),
      subtitle: constant("Workspace"),
      keywords: ["open", "folder", "repository", "project", "directory"],
    }),
  );
  push(
    row({
      commandId: "palette.openFolderInVSCodeInline",
      title: constant("Open Folder in VS Code (Inline)…"),
      subtitle: constant("VS Code Inline"),
      keywords: ["open", "folder", "directory", "project", "vs", "code", "inline", "editor", "browser"],
      when: (c) => c.vscodeInlineOpenTargetAvailable === true,
    }),
  );
  push(
    row({
      commandId: "palette.reopenPreviousSession",
      title: constant("Restore Previous App Launch"),
      subtitle: constant("History"),
      keywords: ["reopen", "restore", "previous", "session", "launch", "resume"],
    }),
  );
  push(
    row({
      commandId: "palette.newTerminalTab",
      title: constant("New Tab (Terminal)"),
      subtitle: constant("Tab"),
      shortcutHint: "⌘T",
      keywords: ["new", "terminal", "tab"],
    }),
  );
  push(
    row({
      commandId: "palette.newBrowserTab",
      title: constant("New Tab (Browser)"),
      subtitle: constant("Tab"),
      shortcutHint: "⌘⇧L",
      keywords: ["new", "browser", "tab", "web"],
      when: (c) => !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.closeTab",
      title: constant("Close Tab"),
      subtitle: constant("Tab"),
      shortcutHint: "⌘W",
      keywords: ["close", "tab"],
    }),
  );
  push(
    row({
      commandId: "palette.closeWorkspace",
      title: constant("Close Workspace"),
      subtitle: constant("Workspace"),
      shortcutHint: "⌘⇧W",
      keywords: ["close", "workspace"],
    }),
  );
  push(
    row({
      commandId: "palette.closeWindow",
      title: constant("Close Window"),
      subtitle: constant("Window"),
      keywords: ["close", "window"],
    }),
  );
  push(
    row({
      commandId: "palette.toggleFullScreen",
      title: constant("Toggle Full Screen"),
      subtitle: constant("Window"),
      keywords: ["fullscreen", "full", "screen", "window", "toggle"],
    }),
  );
  push(
    row({
      commandId: "palette.reopenClosedBrowserTab",
      title: constant("Reopen Last Closed"),
      subtitle: constant("History"),
      keywords: ["reopen", "closed", "recently", "history", "tab", "workspace", "window"],
    }),
  );
  push(
    row({
      commandId: "palette.toggleSidebar",
      title: constant("Toggle Left Sidebar"),
      subtitle: constant("Layout"),
      keywords: ["toggle", "sidebar", "left", "layout"],
    }),
  );
  push(
    row({
      commandId: "palette.toggleFileExplorer",
      title: constant("Toggle File Explorer"),
      subtitle: constant("Layout"),
      keywords: ["toggle", "sidebar", "right", "files", "file", "explorer", "layout"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  splice("extensionSidebar"); // 6473
  splice("rightSidebarMode"); // 6485
  splice("rightSidebarToolPane"); // 6486
  push(
    row({
      commandId: "palette.toggleMatchTerminalBackground",
      title: (c) =>
        c.sidebarMatchTerminalBackground
          ? "Disable Match Terminal Background"
          : "Enable Match Terminal Background",
      subtitle: constant("Sidebar"),
      keywords: ["match", "terminal", "background", "transparency", "sidebar", "surface", "chrome"],
    }),
  );
  push(
    row({
      commandId: "palette.enableMinimalMode",
      title: constant("Enable Minimal Mode"),
      subtitle: constant("Layout"),
      keywords: ["minimal", "mode", "titlebar", "sidebar", "layout"],
      when: (c) => !c.workspaceMinimalModeEnabled,
    }),
  );
  push(
    row({
      commandId: "palette.disableMinimalMode",
      title: constant("Disable Minimal Mode"),
      subtitle: constant("Layout"),
      keywords: ["minimal", "mode", "titlebar", "sidebar", "layout"],
      when: (c) => c.workspaceMinimalModeEnabled === true,
    }),
  );
  // View sub-list (static) — `ContentView+ViewCommandPalette.swift:10-24`.
  push(
    row({
      commandId: "palette.triggerFlash",
      title: constant("Flash Focused Panel"),
      subtitle: constant("View"),
      keywords: ["flash", "highlight", "focus", "panel"],
    }),
  );
  push(
    row({
      commandId: "palette.openTaskManager",
      title: constant("Task Manager"),
      subtitle: constant("Window"),
      keywords: ["task", "manager", "process", "cpu", "memory", "kill"],
    }),
  );
  splice("canvas"); // 6518
  push(
    row({
      commandId: "palette.showNotifications",
      title: constant("Show Notifications"),
      subtitle: constant("Notifications"),
      keywords: ["notifications", "inbox"],
    }),
  );
  push(
    row({
      commandId: "palette.jumpUnread",
      title: constant("Jump to Latest Unread"),
      subtitle: constant("Notifications"),
      keywords: ["jump", "unread", "notification"],
    }),
  );
  push(
    row({
      commandId: "palette.toggleUnread",
      title: constant("Toggle Unread"),
      subtitle: constant("Notifications"),
      keywords: ["toggle", "mark", "read", "unread", "notification"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.markOldestUnreadAndJumpNext",
      title: constant("Mark as Oldest Unread and Jump to Next Latest Unread"),
      subtitle: constant("Notifications"),
      keywords: ["mark", "oldest", "unread", "jump", "next", "notification", "defer"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.openSettings",
      title: constant("Open Settings"),
      subtitle: constant("Global"),
      shortcutHint: "⌘,",
      keywords: ["settings", "preferences"],
    }),
  );
  push(
    row({
      commandId: "palette.openCmuxSettingsFile",
      title: constant("Open cmux.json"),
      subtitle: constant("cmux.json"),
      keywords: ["open", "cmux", "json", "config", "configuration", "settings", "file", "editor", "dotfile"],
    }),
  );
  push(
    row({
      commandId: "palette.openGhosttySettings",
      title: constant("Open Ghostty Settings in TextEdit"),
      subtitle: constant("Ghostty Config Files"),
      keywords: ["open", "ghostty", "settings", "config", "configuration", "file", "textedit", "terminal"],
    }),
  );
  push(
    row({
      commandId: "palette.mobileConnect",
      title: constant("Connect iPhone/iPad"),
      subtitle: constant("Mobile"),
      // Platform/technical terms; not localized (ContentView.swift:6258-6261).
      keywords: [
        "mobile", "connect", "pair", "pairing", "device",
        "ios", "ipados", "iphone", "ipad", "phone", "tablet", "qr",
      ],
    }),
  );
  // Auth sub-list (static) — `ContentView+AuthCommandPalette.swift:14-34`.
  push(
    row({
      commandId: "palette.auth.signIn",
      title: constant("Sign In"),
      subtitle: constant("Account"),
      keywords: ["account", "auth", "authenticate", "authentication", "login", "log in", "signin", "sign in"],
      when: (c) => !c.authSignedIn && !c.authWorking,
    }),
  );
  push(
    row({
      commandId: "palette.auth.signOut",
      title: constant("Sign Out"),
      subtitle: constant("Account"),
      keywords: ["account", "auth", "logout", "log out", "signout", "sign out"],
      when: (c) => c.authSignedIn === true && !c.authWorking,
    }),
  );
  push(
    row({
      commandId: "palette.makeDefaultTerminal",
      title: constant("Make cmux the Default Terminal"),
      subtitle: constant("Global"),
      keywords: ["default", "terminal", "ssh", "launch", "services", "handler", "command", "tool", "executable"],
      when: (c) => !c.defaultTerminalIsDefault,
    }),
  );
  push(
    row({
      commandId: "palette.checkForUpdates",
      title: constant("Check for Updates"),
      subtitle: constant("Global"),
      keywords: ["update", "upgrade", "release"],
    }),
  );
  push(
    row({
      commandId: "palette.applyUpdateIfAvailable",
      title: constant("Apply Update (If Available)"),
      subtitle: constant("Global"),
      keywords: ["apply", "install", "update", "available"],
      when: (c) => c.updateHasAvailable === true,
    }),
  );
  push(
    row({
      commandId: "palette.attemptUpdate",
      title: constant("Attempt Update"),
      subtitle: constant("Global"),
      keywords: ["attempt", "check", "update", "upgrade", "release"],
    }),
  );
  push(
    row({
      commandId: "palette.restartSocketListener",
      title: constant("Restart CLI Listener"),
      subtitle: constant("Global"),
      keywords: ["restart", "socket", "listener", "cli", "cmux", "control"],
    }),
  );
  push(
    row({
      commandId: "palette.disableBrowser",
      title: constant("Disable cmux Browser"),
      subtitle: constant("Browser"),
      keywords: ["browser", "disable", "external", "default", "open", "auth"],
      when: (c) => !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.enableBrowser",
      title: constant("Enable cmux Browser"),
      subtitle: constant("Browser"),
      keywords: ["browser", "enable", "embedded", "open"],
      when: (c) => c.browserDisabled === true,
    }),
  );
  splice("settingsToggle"); // 6672
  push(
    row({
      commandId: "palette.renameWorkspace",
      title: constant("Rename Workspace…"),
      subtitle: workspaceSubtitle,
      keywords: ["rename", "workspace", "title"],
      dismissOnRun: false,
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.editWorkspaceDescription",
      title: constant("Edit Workspace Description…"),
      subtitle: workspaceSubtitle,
      keywords: ["edit", "workspace", "description", "notes", "markdown"],
      dismissOnRun: false,
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.clearWorkspaceName",
      title: constant("Clear Workspace Name"),
      subtitle: workspaceSubtitle,
      keywords: ["clear", "workspace", "name"],
      when: (c) => c.hasWorkspace === true && c.workspaceHasCustomName === true,
    }),
  );
  push(
    row({
      commandId: "palette.clearWorkspaceDescription",
      title: constant("Clear Workspace Description"),
      subtitle: workspaceSubtitle,
      keywords: ["clear", "workspace", "description", "notes"],
      when: (c) => c.hasWorkspace === true && c.workspaceHasCustomDescription === true,
    }),
  );
  push(
    row({
      commandId: "palette.toggleWorkspacePin",
      title: (c) => (c.workspaceShouldPin ? "Pin Workspace" : "Unpin Workspace"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "pin", "pinned"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.resetWorkspaceColor",
      title: constant("Reset Workspace Color"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "color", "reset", "clear", "palette"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  splice("workspaceColors"); // 6738
  push(
    row({
      commandId: "palette.nextWorkspace",
      title: constant("Next Workspace"),
      subtitle: constant("Workspace Navigation"),
      keywords: ["next", "workspace", "navigate"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.previousWorkspace",
      title: constant("Previous Workspace"),
      subtitle: constant("Workspace Navigation"),
      keywords: ["previous", "workspace", "navigate"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.moveWorkspaceUp",
      title: constant("Move Up"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "move", "up", "reorder"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceCanMoveUp === true,
    }),
  );
  push(
    row({
      commandId: "palette.moveWorkspaceDown",
      title: constant("Move Down"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "move", "down", "reorder"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceCanMoveDown === true,
    }),
  );
  push(
    row({
      commandId: "palette.moveWorkspaceToTop",
      title: constant("Move to Top"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "move", "top", "reorder"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceCanMoveUp === true,
    }),
  );
  push(
    row({
      commandId: "palette.closeOtherWorkspaces",
      title: constant("Close Other Workspaces"),
      subtitle: workspaceSubtitle,
      keywords: ["close", "other", "workspaces", "reset", "workspace"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceHasPeers === true,
    }),
  );
  push(
    row({
      commandId: "palette.closeWorkspacesBelow",
      title: constant("Close Workspaces Below"),
      subtitle: workspaceSubtitle,
      keywords: ["close", "below", "workspaces", "workspace"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceHasBelow === true,
    }),
  );
  push(
    row({
      commandId: "palette.closeWorkspacesAbove",
      title: constant("Close Workspaces Above"),
      subtitle: workspaceSubtitle,
      keywords: ["close", "above", "workspaces", "workspace"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceHasAbove === true,
    }),
  );
  push(
    row({
      commandId: "palette.collapseWorkspaceGroup",
      title: constant("Collapse Workspace Group"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "group", "collapse", "sidebar", "folder"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceGroupCanCollapse === true,
    }),
  );
  push(
    row({
      commandId: "palette.expandWorkspaceGroup",
      title: constant("Expand Workspace Group"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "group", "expand", "sidebar", "folder"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceGroupCanExpand === true,
    }),
  );
  push(
    row({
      commandId: "palette.markWorkspaceRead",
      title: constant("Mark Workspace as Read"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "read", "notification", "inbox"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceCanMarkRead === true,
    }),
  );
  push(
    row({
      commandId: "palette.markWorkspaceUnread",
      title: constant("Mark Workspace as Unread"),
      subtitle: workspaceSubtitle,
      keywords: ["workspace", "unread", "notification", "inbox"],
      when: (c) => c.hasWorkspace === true,
      enablement: (c) => c.workspaceCanMarkUnread === true,
    }),
  );
  // Identifier-copy sub-list (static) — `ContentViewIdentifierCopyCommands.swift:15-86`.
  push(
    row({
      commandId: "palette.copyWorkspaceID",
      title: constant("Copy Workspace ID"),
      subtitle: workspaceSubtitle,
      keywords: ["copy", "workspace", "id", "identifier"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.copyWorkspaceIDAndRef",
      title: constant("Copy Workspace ID and Ref"),
      subtitle: workspaceSubtitle,
      keywords: ["copy", "workspace", "id", "identifier", "ref", "reference"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.copyWorkspaceLink",
      title: constant("Copy Workspace Link"),
      subtitle: workspaceSubtitle,
      keywords: ["copy", "workspace", "link", "url", "deeplink", "deep link"],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.copyPaneID",
      title: constant("Copy Pane ID"),
      subtitle: panelSubtitle,
      keywords: ["copy", "pane", "split", "id", "identifier"],
      when: (c) => c.panelHasPane === true,
    }),
  );
  push(
    row({
      commandId: "palette.copyPaneLink",
      title: constant("Copy Pane Link"),
      subtitle: panelSubtitle,
      keywords: ["copy", "pane", "split", "link", "url", "deeplink", "deep link"],
      when: (c) => c.panelHasPane === true,
    }),
  );
  push(
    row({
      commandId: "palette.copySurfaceID",
      title: constant("Copy Surface ID"),
      subtitle: panelSubtitle,
      keywords: ["copy", "surface", "tab", "id", "identifier"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.copySurfaceLink",
      title: constant("Copy Surface Link"),
      subtitle: panelSubtitle,
      keywords: ["copy", "surface", "tab", "link", "url", "deeplink", "deep link"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.copyIdentifiers",
      title: constant("Copy IDs and Refs"),
      subtitle: panelSubtitle,
      keywords: ["copy", "ids", "identifiers", "workspace", "pane", "surface", "ref", "reference"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.renameTab",
      title: constant("Rename Tab…"),
      subtitle: panelSubtitle,
      keywords: ["rename", "tab", "title"],
      dismissOnRun: false,
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.clearTabName",
      title: constant("Clear Tab Name"),
      subtitle: panelSubtitle,
      keywords: ["clear", "tab", "name"],
      when: (c) => c.hasFocusedPanel === true && c.panelHasCustomName === true,
    }),
  );
  // Move-tab sub-list (static) — `ContentView+MoveTabToNewWorkspace.swift:11-19`.
  push(
    row({
      commandId: "palette.moveTabToNewWorkspace",
      title: constant("Move Tab to New Workspace"),
      subtitle: panelSubtitle,
      keywords: ["move", "tab", "workspace", "detach", "sidebar", "surface"],
      when: (c) => c.hasFocusedPanel === true,
      enablement: (c) => c.panelCanMoveToNewWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.toggleTabPin",
      title: (c) => (c.panelShouldPin ? "Pin Tab" : "Unpin Tab"),
      subtitle: panelSubtitle,
      keywords: ["tab", "pin", "pinned"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.toggleTabUnread",
      title: (c) => (c.panelHasUnread ? "Mark Tab as Read" : "Mark Tab as Unread"),
      subtitle: panelSubtitle,
      keywords: ["tab", "read", "unread", "notification"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.nextTabInPane",
      title: constant("Next Tab in Pane"),
      subtitle: constant("Tab Navigation"),
      keywords: ["next", "tab", "pane"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.previousTabInPane",
      title: constant("Previous Tab in Pane"),
      subtitle: constant("Tab Navigation"),
      keywords: ["previous", "tab", "pane"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.openWorkspacePullRequests",
      title: constant("Open All Workspace PR Links"),
      subtitle: workspaceSubtitle,
      keywords: ["pull", "request", "review", "merge", "pr", "mr", "open", "links", "workspace"],
      when: (c) => c.hasWorkspace === true && c.workspaceHasPullRequests === true,
    }),
  );
  push(
    row({
      commandId: "palette.openDiffViewer",
      title: constant("Open Diff Viewer"),
      subtitle: workspaceSubtitle,
      keywords: ["diff", "changes", "git", "review", "branch", "unstaged", "codeview", "agent", "codex", "claude"],
      when: (c) => c.hasWorkspace === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.openDirectoryDiffViewer",
      title: constant("Open Directory Diff Viewer"),
      subtitle: workspaceSubtitle,
      keywords: ["diff", "changes", "git", "review", "branch", "unstaged", "codeview", "directory", "cwd", "folder"],
      when: (c) => c.hasWorkspace === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.browserBack",
      title: constant("Back"),
      subtitle: browserPanelSubtitle,
      shortcutHint: "⌘[",
      keywords: ["browser", "back", "history"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserForward",
      title: constant("Forward"),
      subtitle: browserPanelSubtitle,
      shortcutHint: "⌘]",
      keywords: ["browser", "forward", "history"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserReload",
      title: constant("Reload Page"),
      subtitle: browserPanelSubtitle,
      shortcutHint: "⌘R",
      keywords: ["browser", "reload", "refresh"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserOpenDefault",
      title: constant("Open Current Page in Default Browser"),
      subtitle: browserPanelSubtitle,
      keywords: ["open", "default", "external", "browser"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserFocusAddressBar",
      title: constant("Focus Address Bar"),
      subtitle: browserPanelSubtitle,
      shortcutHint: "⌘L",
      keywords: ["browser", "address", "omnibar", "url"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserFocusMode",
      title: (c) =>
        c.panelBrowserFocusModeActive ? "Exit Browser Focus Mode" : "Enter Browser Focus Mode",
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "focus", "mode", "keyboard", "shortcuts", "webview"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserToggleOmnibar",
      title: (c) =>
        c.panelBrowserOmnibarVisible ? "Hide Browser Omnibar" : "Show Browser Omnibar",
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "address", "omnibar", "url", "toolbar", "chrome", "show", "hide"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserToggleDevTools",
      title: constant("Toggle Developer Tools"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "devtools", "inspector"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserConsole",
      title: constant("Show JavaScript Console"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "console", "javascript"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserReactGrab",
      title: constant("Toggle React Grab"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "react", "grab", "inspect", "element"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserNetwork",
      title: constant("Show Network Requests"),
      subtitle: browserPanelSubtitle,
      keywords: [
        "browser",
        "network",
        "requests",
        "headers",
        "body",
        "status",
        "proxy",
        "webview",
      ],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserNetworkClear",
      title: constant("Clear Network Records"),
      subtitle: browserPanelSubtitle,
      keywords: [
        "browser",
        "network",
        "requests",
        "clear",
        "reset",
        "proxy",
        "webview",
      ],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserZoomIn",
      title: constant("Zoom In"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "zoom", "in"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserZoomOut",
      title: constant("Zoom Out"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "zoom", "out"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserZoomReset",
      title: constant("Actual Size"),
      subtitle: browserPanelSubtitle,
      keywords: ["browser", "zoom", "reset", "actual size"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.markdownZoomIn",
      title: constant("Zoom In"),
      subtitle: markdownPanelSubtitle,
      keywords: ["markdown", "zoom", "in", "font", "size", "bigger", "larger"],
      when: (c) => c.panelIsMarkdown === true,
    }),
  );
  push(
    row({
      commandId: "palette.markdownZoomOut",
      title: constant("Zoom Out"),
      subtitle: markdownPanelSubtitle,
      keywords: ["markdown", "zoom", "out", "font", "size", "smaller"],
      when: (c) => c.panelIsMarkdown === true,
    }),
  );
  push(
    row({
      commandId: "palette.markdownZoomReset",
      title: constant("Actual Size"),
      subtitle: markdownPanelSubtitle,
      keywords: ["markdown", "zoom", "reset", "actual size", "font", "default"],
      when: (c) => c.panelIsMarkdown === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserClearHistory",
      title: constant("Clear Browser History"),
      subtitle: constant("Browser"),
      keywords: ["browser", "history", "clear"],
      when: (c) => c.panelIsBrowser === true,
    }),
  );
  push(
    row({
      commandId: "palette.browserSplitRight",
      title: constant("Split Browser Right"),
      subtitle: constant("Browser Layout"),
      keywords: ["browser", "split", "right"],
      when: (c) => c.panelIsBrowser === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.browserSplitDown",
      title: constant("Split Browser Down"),
      subtitle: constant("Browser Layout"),
      keywords: ["browser", "split", "down"],
      when: (c) => c.panelIsBrowser === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.browserDuplicateRight",
      title: constant("Duplicate Browser to the Right"),
      subtitle: constant("Browser Layout"),
      keywords: ["browser", "duplicate", "clone", "split"],
      when: (c) => c.panelIsBrowser === true && !c.browserDisabled,
    }),
  );
  splice("terminalOpenTargets"); // 7156
  push(
    row({
      commandId: "palette.vscodeServeWebStop",
      title: constant("Stop VS Code Inline Server"),
      subtitle: terminalPanelSubtitle,
      keywords: ["vscode", "inline", "serve-web", "stop", "server"],
      when: (c) => c.panelIsTerminal === true && c.vscodeInlineOpenTargetAvailable === true,
    }),
  );
  push(
    row({
      commandId: "palette.vscodeServeWebRestart",
      title: constant("Restart VS Code Inline Server"),
      subtitle: terminalPanelSubtitle,
      keywords: ["vscode", "inline", "serve-web", "restart", "server"],
      when: (c) => c.panelIsTerminal === true && c.vscodeInlineOpenTargetAvailable === true,
    }),
  );
  push(
    row({
      commandId: "palette.findInDirectory",
      title: constant("Find in Directory…"),
      subtitle: constant("Right Sidebar"),
      keywords: ["files", "directory", "find", "search"],
    }),
  );
  push(
    row({
      commandId: "palette.terminalFind",
      title: constant("Find…"),
      subtitle: terminalPanelSubtitle,
      shortcutHint: "⌘F",
      keywords: ["terminal", "find", "search"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalFindNext",
      title: constant("Find Next"),
      subtitle: terminalPanelSubtitle,
      shortcutHint: "⌘G",
      keywords: ["terminal", "find", "next", "search"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalFindPrevious",
      title: constant("Find Previous"),
      subtitle: terminalPanelSubtitle,
      shortcutHint: "⌥⌘G",
      keywords: ["terminal", "find", "previous", "search"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalHideFind",
      title: constant("Hide Find Bar"),
      subtitle: terminalPanelSubtitle,
      shortcutHint: "⌥⌘⇧F",
      keywords: ["terminal", "hide", "find", "search"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalUseSelectionForFind",
      title: constant("Use Selection for Find"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "selection", "find"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalToggleTextBoxInput",
      title: constant("Toggle TextBox Input"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "textbox", "text", "box", "rich", "input", "prompt"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalFocusTextBoxInput",
      title: constant("Focus TextBox Input"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "textbox", "text", "box", "rich", "input", "prompt", "focus"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalAttachTextBoxFile",
      title: constant("Attach File to TextBox Input"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "textbox", "text", "box", "rich", "input", "attach", "file", "image"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalSendCtrlF",
      title: constant("Send Ctrl-F to Terminal"),
      subtitle: terminalPanelSubtitle,
      keywords: [
        "terminal", "ctrl", "control", "f", "send", "key", "passthrough",
        "force", "stop", "agent", "agents", "claude", "code", "hung", "background", "watchdog", "kill",
      ],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalClearScreenKeepScrollback",
      title: constant("Clear Screen (Keep Scrollback)"),
      subtitle: terminalPanelSubtitle,
      keywords: [
        "terminal", "clear", "screen", "scrollback", "history", "keep",
        "preserve", "reset", "wipe", "cls", "erase",
      ],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.focusLeft",
      title: constant("Focus Pane Left"),
      subtitle: constant("Pane Focus"),
      keywords: ["focus", "pane", "left", "keyboard", "navigation"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.focusRight",
      title: constant("Focus Pane Right"),
      subtitle: constant("Pane Focus"),
      keywords: ["focus", "pane", "right", "keyboard", "navigation"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.focusUp",
      title: constant("Focus Pane Up"),
      subtitle: constant("Pane Focus"),
      keywords: ["focus", "pane", "up", "above", "keyboard", "navigation"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.focusDown",
      title: constant("Focus Pane Down"),
      subtitle: constant("Pane Focus"),
      keywords: ["focus", "pane", "down", "below", "keyboard", "navigation"],
      when: (c) => c.hasFocusedPanel === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalSplitRight",
      title: constant("Split Right"),
      subtitle: constant("Terminal Layout"),
      keywords: ["terminal", "split", "right"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.warmClaudeCode",
      title: constant("Warm Claude Code"),
      subtitle: workspaceSubtitle,
      shortcutHint: "⌃⌥C",
      keywords: [
        "agent",
        "claude",
        "code",
        "warm",
        "pool",
        "prewarm",
        "shortcut",
        "start",
      ],
      when: (c) => c.hasWorkspace === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationRight",
      title: constant("Fork Conversation to the Right"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "right", "split"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationLeft",
      title: constant("Fork Conversation to the Left"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "left", "split"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationTop",
      title: constant("Fork Conversation to the Top"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "top", "up", "above", "split"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationBottom",
      title: constant("Fork Conversation to the Bottom"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "bottom", "down", "below", "split"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationNewTab",
      title: constant("Fork Conversation to New Tab"),
      subtitle: terminalPanelSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "new", "tab", "same", "pane"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.forkAgentConversationNewWorkspace",
      title: constant("Fork Conversation to New Workspace"),
      subtitle: workspaceSubtitle,
      keywords: ["terminal", "agent", "fork", "conversation", "session", "claude", "codex", "opencode", "new", "workspace"],
      when: (c) => c.panelIsTerminal === true && c.panelHasForkableAgent === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalSplitDown",
      title: constant("Split Down"),
      subtitle: constant("Terminal Layout"),
      keywords: ["terminal", "split", "down"],
      when: (c) => c.panelIsTerminal === true,
    }),
  );
  push(
    row({
      commandId: "palette.terminalSplitBrowserRight",
      title: constant("Split Browser Right"),
      subtitle: constant("Terminal Layout"),
      keywords: ["terminal", "split", "browser", "right"],
      when: (c) => c.panelIsTerminal === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.terminalSplitBrowserDown",
      title: constant("Split Browser Down"),
      subtitle: constant("Terminal Layout"),
      keywords: ["terminal", "split", "browser", "down"],
      when: (c) => c.panelIsTerminal === true && !c.browserDisabled,
    }),
  );
  push(
    row({
      commandId: "palette.toggleSplitZoom",
      title: constant("Toggle Pane Zoom"),
      subtitle: constant("Terminal Layout"),
      keywords: ["terminal", "pane", "split", "zoom", "maximize"],
      when: (c) => c.panelIsTerminal === true && c.workspaceHasSplits === true,
    }),
  );
  push(
    row({
      commandId: "palette.equalizeSplits",
      title: constant("Equalize Splits"),
      subtitle: workspaceSubtitle,
      keywords: ["split", "equalize", "balance", "divider", "layout"],
      when: (c) => c.workspaceHasSplits === true,
    }),
  );
  splice("configIssues"); // 7438
  splice("configCustomActions"); // 7448

  return out;
}

// ---------------------------------------------------------------------------
// Build loop + dispatch
// ---------------------------------------------------------------------------

/**
 * Port of `commandPaletteCommands` (`Sources/ContentView.swift:6010-6052`):
 * iterate the contribution list in declared order, drop rows whose config
 * `.palette` gate (6025-6027) or `when`/`enablement` (6028) is false or whose
 * intent cannot be resolved (6029-6032), and assign `rank` = post-filter
 * sequential index (6020/6048 — rank is the index among SURVIVING rows, not the
 * pre-filter position).
 */
export function buildCommandCatalog(
  ctx: CommandContext,
  options: BuildCommandCatalogOptions = {},
): CommandDescriptor[] {
  const { configResolver, dynamicContributions = {} } = options;
  const contributions = buildContributions(dynamicContributions);

  const descriptors: CommandDescriptor[] = [];
  let nextRank = 0;

  for (const contribution of contributions) {
    // Parity with `commandPaletteConfigActionID` (ContentView.swift:6054-6067):
    // the host consults cmux.json ONLY for the four configurable ids; for every
    // other id `commandPaletteConfigActionID` returns nil, so `flatMap` never
    // resolves a config action and the row is never dropped/overridden by
    // config. Gate structurally here so a D4 resolver cannot affect ids the
    // host would never consult.
    const configured = COMMAND_PALETTE_CONFIGURABLE_COMMAND_IDS.includes(
      contribution.commandId,
    )
      ? (configResolver?.(contribution.commandId) ?? null)
      : null;
    if (configured && !configured.palette) {
      continue;
    }
    if (!contribution.when(ctx) || !contribution.enablement(ctx)) {
      continue;
    }
    const intent = contribution.intent ?? resolveIntent(contribution.commandId);
    if (intent === null) {
      // Parity with the `assertionFailure` + `continue` on a missing handler.
      continue;
    }

    const overrideKeywords =
      configured?.keywords !== undefined && configured.keywords.length > 0
        ? configured.keywords
        : contribution.keywords;

    descriptors.push({
      id: contribution.commandId,
      rank: nextRank,
      title: configured?.title ?? contribution.title(ctx),
      subtitle: configured?.subtitle ?? contribution.subtitle(ctx),
      shortcutHint: contribution.shortcutHint,
      kindLabel: null,
      keywords: overrideKeywords,
      dismissOnRun: contribution.dismissOnRun,
      intent,
    });
    nextRank += 1;
  }

  return descriptors;
}

/** The result of dispatching a command: its intent + dismiss gating. */
export interface CommandDispatch {
  intent: CommandIntent;
  dismissOnRun: boolean;
}

/**
 * Port of `runCommandPaletteCommand` (`Sources/ContentView.swift:8499-8523`)
 * MINUS the side effects (usage recording 8504, the `action()` call, and the
 * dismiss timing) — those land in D4. Returns the command's intent and its
 * `dismissOnRun`, or `null` if the command is absent from the catalog for `ctx`
 * (filtered out or unknown id).
 */
export function dispatchCommand(
  commandId: string,
  ctx: CommandContext,
  options: BuildCommandCatalogOptions = {},
): CommandDispatch | null {
  const descriptor = buildCommandCatalog(ctx, options).find((d) => d.id === commandId);
  if (descriptor === undefined) {
    return null;
  }
  return { intent: descriptor.intent, dismissOnRun: descriptor.dismissOnRun };
}
