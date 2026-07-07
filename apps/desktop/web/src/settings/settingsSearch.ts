// Settings search producer — faithful TS re-port of the cmux macOS settings
// search feature.
//
// Two oracles, both mirrored here:
//   1. The scorer + alias tables + builders are a byte-faithful re-port of the
//      already-ported Rust crate `crates/cmux-settings-search`
//      (`index.rs`, `aliases.rs`, `entry.rs`, `target.rs`), which itself ports
//      `Sources/SettingsSearchAliases.swift` (179-319) and
//      `Sources/SettingsNavigation.swift` (the two keyed enums).
//   2. The corpus (`sectionEntries` / `settingEntries` / `allEntries`) and the
//      producer entrypoint `entries(matching:)` are re-ported directly from
//      `Sources/SettingsNavigation.swift:304-590` + the one extra row in
//      `Sources/SettingsNavigation+TerminalScrollSpeed.swift`, because the crate
//      deliberately does NOT port the corpus or the producer.
//
// This module is PURE: no Tauri, DOM, or globals. Wiring the producer into the
// Settings search box is a LATER slice.
//
// Sanctioned platform divergences (all bounded to the ASCII/English domain every
// in-repo title/alias/query lives in — identical behaviour for all real data):
//   - `normalized` ports Foundation's diacritic-insensitive case-folding via a
//     lowercase pass plus a curated precomposed-Latin -> base map and a combining
//     -mark drop (U+0300..U+036F). It intentionally does NOT use
//     `String.prototype.normalize("NFD")`: that would over-strip (e.g. fold marks
//     the crate leaves untouched) and would not match the crate's curated map.
//     Full case folding (`ß` -> `ss`) is not reproduced (neither Rust nor JS
//     lowercasing folds it), and non-Latin scripts (Han, etc.) are kept verbatim.
//   - The token delimiter set is the exact ASCII subset of the Unicode P
//     categories the crate hardcodes (EXCLUDING the ASCII symbols `$ + < = > ^
//     \` | ~`, which are Sc/Sm/Sk, so `+` and `=` STAY inside tokens). Non-ASCII
//     is never a delimiter. We do NOT use a `\p{P}` regex (it would over-match).
//   - The scalar helpers iterate by Unicode codepoint (`[...str]`), not UTF-16
//     units and not grapheme clusters, matching the crate's `char` semantics.
//   - `localized(...)` collapses to the English `defaultValue` verbatim (web i18n
//     is out of scope for this pure producer).
//   - `keyboardShortcutActionAliases` is empty headless, so the
//     `keyboardShortcuts:shortcuts` aliases carry a trailing space, exactly as the
//     crate reproduces (Swift joins live action labels; the web has none yet).

// ----------------------------------------------------------------------------
// SettingsNavigationTarget (port of `Sources/SettingsNavigation.swift:3-133`)
// ----------------------------------------------------------------------------

/**
 * The top-level settings section a search entry navigates to. Mirrors Swift's
 * `enum SettingsNavigationTarget` (a `String`-backed `CaseIterable`).
 *
 * DIVERGENCE (as in `appearanceMode.ts`): the string literal union values ARE
 * the Swift `rawValue`s (case names, camelCase preserved — `textBox`,
 * `sidebarAppearance`, `settingsJSON`), so `rawValue(target) === target` and no
 * separate mapping is needed.
 */
export type SettingsNavigationTarget =
  | "account"
  | "app"
  | "terminal"
  | "textBox"
  | "mobile"
  | "sidebarAppearance"
  | "customSidebars"
  | "betaFeatures"
  | "automation"
  | "browser"
  | "browserImport"
  | "globalHotkey"
  | "keyboardShortcuts"
  | "workspaceColors"
  | "settingsJSON"
  | "reset";

/** `SettingsNavigationTarget.allCases` order (Swift case order). */
export const SETTINGS_NAVIGATION_TARGETS: readonly SettingsNavigationTarget[] = [
  "account",
  "app",
  "terminal",
  "textBox",
  "mobile",
  "sidebarAppearance",
  "customSidebars",
  "betaFeatures",
  "automation",
  "browser",
  "browserImport",
  "globalHotkey",
  "keyboardShortcuts",
  "workspaceColors",
  "settingsJSON",
  "reset",
];

/** `var title` (Swift lines 23-58). English `defaultValue` verbatim. */
const TARGET_TITLE: Record<SettingsNavigationTarget, string> = {
  account: "Account",
  app: "App",
  terminal: "Terminal",
  textBox: "TextBox (Beta)",
  mobile: "Mobile",
  workspaceColors: "Workspace Colors",
  sidebarAppearance: "Sidebar",
  customSidebars: "Custom Sidebars",
  betaFeatures: "Beta Features",
  automation: "Automation",
  browser: "Browser",
  browserImport: "Import Browser Data",
  globalHotkey: "Global Hotkey",
  keyboardShortcuts: "Keyboard Shortcuts",
  settingsJSON: "cmux.json",
  reset: "Reset",
};

/** `var symbolName` (Swift lines 60-95). SF Symbol name verbatim. */
const TARGET_SYMBOL_NAME: Record<SettingsNavigationTarget, string> = {
  account: "person.crop.circle",
  app: "gearshape",
  terminal: "terminal",
  textBox: "textformat",
  mobile: "iphone",
  workspaceColors: "paintpalette",
  sidebarAppearance: "sidebar.left",
  customSidebars: "sidebar.squares.left",
  betaFeatures: "exclamationmark.triangle",
  automation: "wand.and.sparkles",
  browser: "globe",
  browserImport: "square.and.arrow.down",
  globalHotkey: "keyboard.badge.ellipsis",
  keyboardShortcuts: "keyboard",
  settingsJSON: "doc.text",
  reset: "arrow.counterclockwise",
};

/**
 * The tail of `var searchText` (Swift lines 97-132). The Swift property
 * interpolates the section `title` at the front (`"\(title) ..."`); see
 * `targetSearchText`.
 */
const TARGET_SEARCH_TEXT_TAIL: Record<SettingsNavigationTarget, string> = {
  account: "sign in team sync",
  app: "appearance language workspace notifications menu bar telemetry default terminal",
  terminal:
    "scrollbar auto resume restore reopen relaunch quit sessions agents claude codex opencode rovodev hibernation idle suspend commands approvals prefixes toggle",
  textBox:
    "textbox text box rich input prompt beta new terminal workspace split tab focus height",
  mobile: "ios iphone ipad mobile pairing local network sync",
  workspaceColors: "palette tabs",
  sidebarAppearance:
    "sidebar details branches badges material terminal background",
  customSidebars:
    "custom sidebars vibe swift json interpreted renderer in-process remote worker isolated",
  betaFeatures: "beta experimental unstable feed dock right sidebar",
  automation:
    "socket integrations hooks ports claude cursor gemini kiro naming auto naming workspace tabs",
  browser: "search engine links history theme",
  browserImport: "browser import data bookmarks history cookies",
  globalHotkey: "system wide shortcut",
  keyboardShortcuts: "keybindings commands chords",
  settingsJSON:
    "config file preferences editor documentation schema jsonc reload",
  reset: "defaults",
};

/** `var title`. */
export function targetTitle(target: SettingsNavigationTarget): string {
  return TARGET_TITLE[target];
}

/** `var symbolName`. */
export function targetSymbolName(target: SettingsNavigationTarget): string {
  return TARGET_SYMBOL_NAME[target];
}

/**
 * `var searchText` — reproduces the Swift `"\(title) ..."` interpolation by
 * prepending the section title, exactly as `target.rs::search_text` does.
 */
export function targetSearchText(target: SettingsNavigationTarget): string {
  return `${targetTitle(target)} ${TARGET_SEARCH_TEXT_TAIL[target]}`;
}

/** Reverse of the identity `rawValue`, mirroring `SettingsNavigationTarget(rawValue:)`. */
export function targetFromRawValue(
  raw: string,
): SettingsNavigationTarget | null {
  return SETTINGS_NAVIGATION_TARGETS.includes(raw as SettingsNavigationTarget)
    ? (raw as SettingsNavigationTarget)
    : null;
}

// ----------------------------------------------------------------------------
// normalized / tokenizer (port of `index.rs:81-118`, `328-400`)
// ----------------------------------------------------------------------------

// Curated precomposed lowercase-Latin -> base map (`index.rs::strip_diacritic`,
// lines 376-400). `normalized` lowercases first, so only lowercase forms appear.
const STRIP_DIACRITIC: Record<string, string> = {};
for (const [base, forms] of [
  ["a", "àáâãäåāăą"],
  ["c", "çćĉċč"],
  ["d", "ðďđ"],
  ["e", "èéêëēĕėęě"],
  ["g", "ĝğġģ"],
  ["h", "ĥħ"],
  ["i", "ìíîïĩīĭįı"],
  ["j", "ĵ"],
  ["k", "ķ"],
  ["l", "ĺļľŀł"],
  ["n", "ñńņň"],
  ["o", "òóôõöøōŏő"],
  ["r", "ŕŗř"],
  ["s", "śŝşš"],
  ["t", "ţťŧ"],
  ["u", "ùúûüũūŭůűų"],
  ["w", "ŵ"],
  ["y", "ýÿŷ"],
  ["z", "źżž"],
] as const) {
  for (const form of forms) {
    STRIP_DIACRITIC[form] = base;
  }
}

/** Unicode combining diacritical marks (U+0300..U+036F), dropped by `normalized`. */
function isCombiningMark(codepoint: number): boolean {
  return codepoint >= 0x0300 && codepoint <= 0x036f;
}

/**
 * ASCII-range membership of the Unicode general punctuation categories
 * (Pc, Pd, Ps, Pe, Pi, Pf, Po). Mirrors `index.rs::is_unicode_punctuation`.
 * Notably EXCLUDES the ASCII symbols `$ + < = > ^ \` | ~` (Sc/Sm/Sk). Non-ASCII
 * returns false.
 */
const ASCII_PUNCTUATION = new Set<string>([
  "!",
  '"',
  "#",
  "%",
  "&",
  "'",
  "(",
  ")",
  "*",
  ",",
  "-",
  ".",
  "/",
  ":",
  ";",
  "?",
  "@",
  "\\",
  "[",
  "]",
  "_",
  "{",
  "}",
]);

/** `char::is_whitespace` — Unicode whitespace. */
function isWhitespace(character: string): boolean {
  return /\s/u.test(character);
}

/** Mirrors `index.rs::is_token_delimiter`. */
function isTokenDelimiter(character: string): boolean {
  return isWhitespace(character) || ASCII_PUNCTUATION.has(character);
}

/**
 * `normalized(_:)` (Swift line 180; `index.rs:81-94`). Lowercase, drop combining
 * marks, strip curated precomposed Latin diacritics. See the module divergence
 * note about the Foundation-folding approximation.
 */
export function normalized(text: string): string {
  const lowered = text.toLowerCase();
  let result = "";
  for (const character of lowered) {
    const codepoint = character.codePointAt(0) ?? 0;
    if (isCombiningMark(codepoint)) {
      continue;
    }
    result += STRIP_DIACRITIC[character] ?? character;
  }
  return result;
}

/**
 * `normalizedTokens(for:)` (Swift lines 201-210; `index.rs:101-118`). Re-folds
 * the query, then splits on runs of whitespace/punctuation, dropping empties.
 */
export function normalizedTokens(query: string): string[] {
  const folded = normalized(query);
  const tokens: string[] = [];
  let current = "";
  for (const character of folded) {
    if (isTokenDelimiter(character)) {
      if (current.length > 0) {
        tokens.push(current);
        current = "";
      }
    } else {
      current += character;
    }
  }
  if (current.length > 0) {
    tokens.push(current);
  }
  return tokens;
}

/** `isSearchStopWord(_:)` (Swift lines 216-223; `index.rs:130-132`). */
export function isSearchStopWord(token: string): boolean {
  return (
    token === "setting" ||
    token === "settings" ||
    token === "preference" ||
    token === "preferences"
  );
}

/** `normalizedQueryTokens(for:)` (Swift lines 212-214; `index.rs:122-127`). */
export function normalizedQueryTokens(query: string): string[] {
  return normalizedTokens(query).filter((token) => !isSearchStopWord(token));
}

// ----------------------------------------------------------------------------
// scalar helpers (port of `index.rs:220-322`)
// ----------------------------------------------------------------------------

// `char::is_alphabetic` / `char::is_numeric` (Unicode). ASCII-identical.
const UNICODE_ALPHABETIC = /\p{Alphabetic}/u;
const UNICODE_NUMERIC = /\p{N}/u;

/** Index of `needle` (codepoint array) in `haystack` at or after `from`, else -1. */
function indexOfCodepoints(
  haystack: readonly string[],
  needle: readonly string[],
  from: number,
): number {
  const limit = haystack.length - needle.length;
  for (let start = from; start <= limit; start++) {
    let matched = true;
    for (let offset = 0; offset < needle.length; offset++) {
      if (haystack[start + offset] !== needle[offset]) {
        matched = false;
        break;
      }
    }
    if (matched) {
      return start;
    }
  }
  return -1;
}

/**
 * `containsAtWordBoundary(_:in:)` (Swift lines 261-275; `index.rs:220-242`).
 * True when `needle` appears at the start or immediately after a character that
 * is neither a letter nor a number. An empty needle is a boundary match. Mirrors
 * the crate's advance-past-the-whole-match on a non-boundary hit.
 */
export function containsAtWordBoundary(
  needle: string,
  haystack: string,
): boolean {
  if (needle.length === 0) {
    return true;
  }
  const needleChars = [...needle];
  const haystackChars = [...haystack];
  let searchStart = 0;
  for (;;) {
    const matchStart = indexOfCodepoints(
      haystackChars,
      needleChars,
      searchStart,
    );
    if (matchStart === -1) {
      return false;
    }
    if (matchStart === 0) {
      return true;
    }
    const previous = haystackChars[matchStart - 1];
    if (!UNICODE_ALPHABETIC.test(previous) && !UNICODE_NUMERIC.test(previous)) {
      return true;
    }
    // Advance past the whole match, matching Swift's `range.upperBound`.
    searchStart = matchStart + needleChars.length;
  }
}

/**
 * `isSubsequence(_:of:)` (Swift lines 277-285; `index.rs:247-265`). True when
 * `needle`'s codepoints appear in order (not necessarily contiguously). An empty
 * needle is always a subsequence.
 */
export function isSubsequence(needle: string, haystack: string): boolean {
  if (needle.length === 0) {
    return true;
  }
  const needleChars = [...needle];
  let index = 0;
  for (const character of haystack) {
    if (character === needleChars[index]) {
      index += 1;
      if (index === needleChars.length) {
        return true;
      }
    }
  }
  return false;
}

/**
 * `editDistance(_:_:maximum:)` (Swift lines 296-318; `index.rs:288-322`).
 * Row-based Levenshtein over codepoints, with both crate early exits preserved:
 * a length delta above `maximum` short-circuits to `maximum + 1`, and any full
 * row whose minimum exceeds `maximum` short-circuits to `maximum + 1`.
 */
export function editDistance(
  lhs: string,
  rhs: string,
  maximum: number,
): number {
  const left = [...lhs];
  const right = [...rhs];
  const leftLen = left.length;
  const rightLen = right.length;

  if (Math.abs(leftLen - rightLen) > maximum) {
    return maximum + 1;
  }

  let previous: number[] = [];
  for (let index = 0; index <= rightLen; index++) {
    previous.push(index);
  }
  let current: number[] = new Array<number>(rightLen + 1).fill(0);

  for (let leftIndex = 1; leftIndex <= leftLen; leftIndex++) {
    current[0] = leftIndex;
    let rowMinimum = current[0];
    for (let rightIndex = 1; rightIndex <= rightLen; rightIndex++) {
      const cost = left[leftIndex - 1] === right[rightIndex - 1] ? 0 : 1;
      current[rightIndex] = Math.min(
        previous[rightIndex] + 1,
        current[rightIndex - 1] + 1,
        previous[rightIndex - 1] + cost,
      );
      rowMinimum = Math.min(rowMinimum, current[rightIndex]);
    }
    if (rowMinimum > maximum) {
      return maximum + 1;
    }
    const swap = previous;
    previous = current;
    current = swap;
  }

  return previous[rightLen];
}

/**
 * `isLightTypo(_:comparedTo:)` (Swift lines 287-294; `index.rs:271-282`). Both
 * strings must be at least 4 codepoints; the allowed distance is 2 when the
 * shorter is at least 6, else 1; the length delta must not exceed the allowance;
 * then the bounded edit distance must be within the allowance.
 */
export function isLightTypo(token: string, word: string): boolean {
  const tokenCount = [...token].length;
  const wordCount = [...word].length;
  if (tokenCount < 4 || wordCount < 4) {
    return false;
  }
  const allowedDistance = Math.min(tokenCount, wordCount) >= 6 ? 2 : 1;
  if (Math.abs(tokenCount - wordCount) > allowedDistance) {
    return false;
  }
  return editDistance(token, word, allowedDistance) <= allowedDistance;
}

// ----------------------------------------------------------------------------
// aliases (port of `aliases.rs`)
// ----------------------------------------------------------------------------

/** `sectionAliases(for:)` (Swift lines 4-39; `aliases.rs:28-77`). */
const SECTION_ALIASES: Record<SettingsNavigationTarget, string> = {
  account:
    "auth authentication login logout sign in sign out email user profile team",
  app: "general preferences prefs behavior chrome dock menubar menu bar status notifications telemetry",
  terminal: "shell scrollback scrollbar scroll bar ghostty tty pty",
  textBox:
    "textbox text box rich input prompt beta focus composer compose attachments",
  mobile: "ios iphone ipad mobile pairing local network permission sync",
  sidebarAppearance:
    "sidebar left rail navigation details branches badges material terminal background",
  customSidebars:
    "custom sidebars vibe code swift json interpreted renderer in-process remote worker isolated",
  betaFeatures: "beta experimental unstable preview feed dock right sidebar",
  automation: "api cli control socket mcp agents hooks ports",
  browser: "web webview address bar omnibar links urls embedded default browser",
  browserImport:
    "chrome safari firefox brave edge arc bookmarks history cookies profiles",
  globalHotkey: "system shortcut global keyboard show hide bring forward",
  keyboardShortcuts:
    "keybinds key bindings hotkeys chords accelerators commands",
  workspaceColors: "tab colors palette accent badge selected highlight",
  settingsJSON:
    "configuration config file json jsonc dotfile ~/.config schema docs",
  reset: "factory defaults restore clear preferences",
};

/** `sectionAliases(for:)`. */
export function sectionAliases(target: SettingsNavigationTarget): string {
  return SECTION_ALIASES[target];
}

/**
 * `settingAliases` (Swift lines 49-168; `aliases.rs:104-433`). The
 * `[String: String]` map keyed by `"<rawValue>:<idSuffix>"`. Returns `undefined`
 * for absent keys (the caller falls back to `""`).
 */
const SETTING_ALIASES: Record<string, string> = {
  "account:account":
    "auth authentication login logout signin sign-in signout sign-out email user profile stack team",
  "app:language":
    "app.language locale l10n localization translation japanese english ja en nihongo restart",
  "app:appearance":
    "app.appearance theme color scheme light mode dark mode system mode",
  "app:app-icon":
    "app.appIcon dock icon application icon app switcher alternate icon",
  "app:default-terminal":
    "app.defaultTerminal default terminal ssh links command tool unix executable launch services handler",
  "app:new-workspace-placement":
    "app.newWorkspacePlacement new tab insert position order top bottom end",
  "app:workspace-group-new-workspace-placement":
    "workspaceGroups.newWorkspacePlacement group new workspace command n cmd-n plus insert position after current top end",
  "app:fork-conversation-default":
    "app.forkConversationDefaultDestination fork conversation right left top bottom split tab workspace default",
  "app:workspace-inherit-working-directory":
    "app.workspaceInheritWorkingDirectory workspace cwd directory inherit current focused ghostty working-directory",
  "app:minimal-mode":
    "app.minimalMode minimal layout simple chrome compact titlebar controls",
  "app:keep-workspace-open":
    "app.keepWorkspaceOpenWhenClosingLastSurface close last pane surface keep tab workspace",
  "app:focus-pane-first-click":
    "app.focusPaneOnFirstClick click to focus focus follows mouse first click mouse activation",
  "app:preferred-editor":
    "app.preferredEditor editor open file code vscode visual studio zed sublime subl cursor",
  "app:supported-file-previews":
    "app.openSupportedFilesInCmux cmd click file preview pdf image video audio quicklook quick look editor external",
  "app:terminal-config":
    "ghostty config configuration terminal settings preview merged file reload macos-option-as-alt option as alt left option right option alt key meta",
  "app:markdown-viewer":
    "app.openMarkdownInCmuxViewer md markdown mdx viewer preview readme",
  "app:markdown-font-size":
    "markdown.fontSize md markdown viewer font size points zoom scale text bigger smaller larger default",
  "app:markdown-font-family":
    "markdown.fontFamily md markdown viewer font font-family family typeface system stack custom",
  "app:markdown-max-width":
    "markdown.maxWidth md markdown viewer max width column reading line length pixels px narrow wide",
  "app:file-editor-word-wrap":
    "fileEditor.wordWrap file editor word wrap soft wrap reflow lines text horizontal scroll preview",
  "app:imessage-mode":
    "app.iMessageMode imessage message messages chat prompt prompts submitted message texting reorder move workspace top agent send",
  "app:reorder-notification":
    "app.reorderOnNotification notification reorder move workspace top unread sort",
  "app:dock-badge":
    "notifications.dockBadge badge dock unread count icon notifications red bubble",
  "app:menu-bar-only":
    "app.menuBarOnly menubar menu bar dockless hide dock app switcher cmd-tab command-tab",
  "app:show-menu-bar":
    "notifications.showInMenuBar menubar menu bar status item tray extra",
  "app:unread-pane-ring":
    "notifications.unreadPaneRing blue border unread ring notification pane outline",
  "app:pane-flash":
    "notifications.paneFlash flash blink highlight pane notification pulse",
  "app:desktop-notifications":
    "macos desktop notifications system settings permission alerts notify test",
  "app:notification-sound":
    "notifications.sound notifications.customSoundFilePath sound audio alert chime beep custom file wav mp3 caf aiff",
  "app:notification-command":
    "notifications.command shell command hook script env environment variable variables done agent",
  "app:telemetry":
    "app.sendAnonymousTelemetry analytics crash reports sentry posthog usage anonymous privacy",
  "app:warn-before-quit":
    "app.warnBeforeQuit quit confirmation command-q cmd-q exit close app",
  "app:warn-before-closing-tab":
    "app.warnBeforeClosingTab close tab confirmation command-w cmd-w terminal surface",
  "app:warn-before-closing-tab-x-button":
    "app.warnBeforeClosingTabXButton close tab x button confirmation terminal surface",
  "app:hide-tab-close-button":
    "app.hideTabCloseButton hide close tab x button terminal surface",
  "app:rename-selects-name":
    "app.renameSelectsExistingName rename select all existing title command palette workspace name",
  "app:palette-search-all":
    "app.commandPaletteSearchesAllSurfaces command palette search all surfaces cmd-p terminal browser markdown",
  "app:canvas-pane-gap":
    "canvas.paneGap canvas pane gap spacing freeform layout panes snapping tidy distribute align",
  "app:canvas-snapping":
    "canvas.snappingEnabled canvas snap snapping enabled edges drag resize align panes freeform layout",
  "terminal:scrollbar":
    "terminal.showScrollBar scrollback scrollbar scroll bar right edge alternate screen tui",
  "terminal:copy-on-select":
    "terminal.copyOnSelect copy on selection select clipboard mouse double click triple click iterm",
  "terminal:tab-bar-font-size":
    "surface-tab-bar-font-size tab bar font size text scale terminal browser pane tab title",
  "terminal:resume-commands":
    "surface resume commands approvals command prefixes auto restore ask manual tmux hibernation sticky process",
  "textBox:show-textbox-new-terminals":
    "terminal.showTextBoxOnNewTerminals show textbox text box rich input prompt default new terminal workspace split tab beta",
  "textBox:focus-textbox-new-terminals":
    "terminal.focusTextBoxOnNewTerminals focus textbox text box rich input prompt default new terminal workspace split tab beta",
  "textBox:textbox-max-lines":
    "terminal.textBoxMaxLines textbox text box rich input prompt max height lines grow scroll beta",
  "sidebarAppearance:match-terminal":
    "sidebarAppearance.matchTerminalBackground transparent background material terminal background sync",
  "sidebarAppearance:font-size":
    "sidebar-font-size sidebar font size text scale workspace title badge metadata shortcut hint",
  "sidebarAppearance:hide-sidebar-details":
    "sidebar.hideAllDetails compact sidebar hide details only title minimal left rail",
  "sidebarAppearance:wrap-workspace-titles":
    "sidebar.wrapWorkspaceTitles workspace title wrap multiline pr pull request",
  "sidebarAppearance:show-workspace-description":
    "sidebar.showWorkspaceDescription workspace description notes markdown sidebar",
  "sidebarAppearance:sidebar-branch-layout":
    "sidebar.branchLayout git branch layout vertical inline cwd directory",
  "sidebarAppearance:stack-branch-directory":
    "sidebar.stackBranchDirectory git branch directory cwd path stack stacked separate lines two rows",
  "sidebarAppearance:path-last-segment-only":
    "sidebar.pathLastSegmentOnly cwd path directory last segment basename short truncate folder repo",
  "sidebarAppearance:show-notification-message":
    "sidebar.showNotificationMessage latest message unread notification text sidebar",
  "sidebarAppearance:show-branch-directory":
    "sidebar.showBranchDirectory git branch cwd path directory folder repo sidebar",
  "sidebarAppearance:show-pull-requests":
    "sidebar.showPullRequests pr mr review github gitlab bitbucket pull request merge request",
  "sidebarAppearance:watch-git-status":
    "sidebar.watchGitStatus git status branch watcher index lock",
  "sidebarAppearance:make-pr-clickable":
    "sidebar.makePullRequestsClickable clickable pull requests pr mr reviews links select workspace row",
  "sidebarAppearance:open-pr-links":
    "sidebar.openPullRequestLinksInCmuxBrowser pr links github browser default external embedded",
  "sidebarAppearance:open-port-links":
    "sidebar.openPortLinksInCmuxBrowser ports localhost links browser default external embedded",
  "sidebarAppearance:show-ssh": "sidebar.showSSH remote host target ssh server",
  "sidebarAppearance:show-ports":
    "sidebar.showPorts localhost port listener dev server url",
  "sidebarAppearance:show-log":
    "sidebar.showLog log status latest message imperative",
  "sidebarAppearance:show-progress":
    "sidebar.showProgress progress bar percent status set_progress",
  "sidebarAppearance:show-metadata":
    "sidebar.showCustomMetadata metadata meta report_meta status custom block",
  "sidebarAppearance:right-max-width":
    "sidebar.rightMaxWidth dock right sidebar max width terminal reservation cap logs lazygit",
  "betaFeatures:feed":
    "feed right sidebar agent decisions permissions questions approval beta unstable",
  "betaFeatures:dock": "dock right sidebar terminal controls tui beta unstable",
  "mobile:iOSPairingHost":
    "ios iphone ipad mobile pairing local network permission sync",
  "mobile:iOSPairingPort":
    "mobile ios iphone pairing port tcp listener firewall conflict bind",
  "mobile:iOSPairingDisplayName":
    "mobile ios iphone pairing display name mac hostname device label",
  "automation:socket-mode":
    "automation.socketControlMode api socket unix domain control server auth allow password disabled",
  "automation:socket-password":
    "automation.socketPassword auth token credential secret password access key",
  "automation:claude-code":
    "automation.claudeCodeIntegration claude code hooks agent integration status notifications",
  "automation:claude-path":
    "automation.claudeBinaryPath claude binary executable path cli command custom",
  "automation:workspace-auto-naming":
    "automation.workspaceAutoNaming automation.autoNamingAgent ai auto naming auto-name auto name workspace tab workspaces tabs title titles rename workspace rename tab renaming generated name summarize summary summarizer conversation agent picker naming agent",
  "automation:ripgrep-path":
    "automation.ripgrepBinaryPath ripgrep rg binary executable path search find nix custom",
  "automation:subagent-notifications":
    "automation.suppressSubagentNotifications subagent nested child agent codex claude hooks notifications",
  "automation:cursor":
    "automation.cursorIntegration cursor ide agent hooks notifications",
  "automation:gemini":
    "automation.geminiIntegration gemini cli google agent hooks notifications",
  "automation:kiro":
    "automation.kiroIntegration kiro cli amazon q agent hooks notifications",
  "automation:kiro-notification-level":
    "automation.kiroNotificationLevel kiro cli notification verbosity minimal standard verbose tool events",
  "automation:port-base":
    "automation.portBase cmux_port start first base env environment variable",
  "automation:port-range":
    "automation.portRange cmux_port_end range size count env ports",
  "browser:enable-browser":
    "browser.enabled enable disable webview embedded browser tabs links",
  "browser:search-engine":
    "browser.defaultSearchEngine browser.customSearchEngineName browser.customSearchEngineURLTemplate omnibar address bar google duckduckgo bing kagi brave startpage perplexity exa yahoo ecosia qwant mojeek wikipedia github baidu yandex custom search provider",
  "browser:search-suggestions":
    "browser.showSearchSuggestions suggest autocomplete address bar search suggestions",
  "browser:theme":
    "browser.theme web page theme color scheme light dark system",
  "browser:hidden-webview-discard":
    "browser.discardHiddenWebViews memory hidden tabs webview discard unload reclaim",
  "browser:hidden-webview-discard-delay":
    "browser.hiddenWebViewDiscardDelaySeconds memory hidden tabs delay seconds discard unload",
  "browser:terminal-links":
    "browser.openTerminalLinksInCmuxBrowser click url terminal links open in browser href",
  "browser:intercept-open":
    "browser.interceptTerminalOpenCommandInCmuxBrowser open command http https url terminal intercept",
  "browser:host-whitelist":
    "browser.hostsToOpenInEmbeddedBrowser allowlist whitelist host wildcard domain embedded browser",
  "browser:external-patterns":
    "browser.urlsToAlwaysOpenExternally denylist blocklist regex rules external default browser",
  "browser:http-allowlist":
    "browser.insecureHttpHostsAllowedInEmbeddedBrowser insecure http allowlist localhost localtest non-https warning",
  "browserImport:import-data":
    "chrome safari firefox brave edge arc bookmarks history cookies profiles migration",
  "browserImport:import-hint":
    "browser.showImportHintOnBlankTabs blank tab onboarding hint import prompt dismiss",
  "browser:react-grab":
    "browser.reactGrabVersion react grab npm version toolbar cmd-shift-g inspect component",
  "browser:history":
    "clear browser history visited pages suggestions omnibar",
  "globalHotkey:enable-hotkey":
    "global hotkey enable system wide show hide all windows",
  "globalHotkey:shortcut":
    "global hotkey shortcut recorder key command option control",
  "keyboardShortcuts:shortcut-chords":
    "tmux prefix ctrl-b control-b multi key sequence chord cmux json",
  "keyboardShortcuts:reset-defaults":
    "reset restore default defaults built in builtin shortcuts hotkeys keybindings commands",
  "keyboardShortcuts:shortcuts":
    "hotkeys keybindings key bindings commands keyboard accelerators shortcuts cmux json",
  "workspaceColors:indicator":
    "workspaceColors.indicatorStyle tab indicator active workspace style color stripe dot",
  "workspaceColors:selection":
    "workspaceColors.selectionColor selected workspace color highlight background active tab",
  "workspaceColors:badge":
    "workspaceColors.notificationBadgeColor unread notification badge color dot count",
  "workspaceColors:palette":
    "workspaceColors.colors workspace palette named colors custom color reset built-in",
  "settingsJSON:open-file":
    "open config file json jsonc config editor ~/.config cmux preferences",
  "settingsJSON:documentation":
    "docs documentation schema reference cmux json keys configuration",
  "reset:reset-all": "factory reset restore defaults clear preferences",
};

/**
 * `settingAliases[key]` lookup (subscript). Returns `null` for absent keys,
 * mirroring the crate's `Option<&str>` / Swift's `nil`.
 */
export function settingAliases(key: string): string | null {
  return Object.prototype.hasOwnProperty.call(SETTING_ALIASES, key)
    ? SETTING_ALIASES[key]
    : null;
}

/**
 * `keyboardShortcutActionAliases` (Swift line 170). Empty headless — see the
 * module divergence note.
 */
function keyboardShortcutActionAliases(): string {
  return "";
}

/**
 * `aliases(target:idSuffix:)` (Swift lines 41-47; `aliases.rs:83-90`). Looks up
 * the `"<rawValue>:<idSuffix>"` key (missing -> `""`) and, for the
 * `keyboardShortcuts:shortcuts` row, appends the (empty headless) action labels
 * — preserving the trailing space from the Swift interpolation.
 */
export function aliases(
  target: SettingsNavigationTarget,
  idSuffix: string,
): string {
  const key = `${target}:${idSuffix}`;
  const value = settingAliases(key) ?? "";
  if (target === "keyboardShortcuts" && idSuffix === "shortcuts") {
    return `${value} ${keyboardShortcutActionAliases()}`;
  }
  return value;
}

// ----------------------------------------------------------------------------
// SettingsSearchEntry + builders (port of `entry.rs`, `index.rs:41-77`)
// ----------------------------------------------------------------------------

/** `enum SettingsSearchEntryKind` (Swift lines 263-266). */
export type SettingsSearchEntryKind = "section" | "setting";

/**
 * `struct SettingsSearchEntry` (Swift lines 268-299). The three `normalized*`
 * fields are precomputed exactly as the Swift initializer does.
 */
export interface SettingsSearchEntry {
  id: string;
  kind: SettingsSearchEntryKind;
  target: SettingsNavigationTarget;
  title: string;
  subtitle: string | null;
  symbolName: string;
  normalizedSearchText: string;
  normalizedSearchWords: string[];
  normalizedSearchWordSet: Set<string>;
}

/**
 * `SettingsSearchEntry.init` (Swift lines 279-298; `entry.rs:52-83`). The
 * combined string is `"<title> <subtitle ?? ""> <searchText>"`; a `null`
 * subtitle produces a doubled space, preserved byte-for-byte (the tokenizer
 * collapses it, but the raw `normalizedSearchText` — which the `.startsWith`
 * / `.includes` bonuses inspect — carries it).
 */
function makeEntry(
  id: string,
  kind: SettingsSearchEntryKind,
  target: SettingsNavigationTarget,
  title: string,
  subtitle: string | null,
  symbolName: string,
  searchText: string,
): SettingsSearchEntry {
  const combined = `${title} ${subtitle ?? ""} ${searchText}`;
  const normalizedSearchText = normalized(combined);
  const normalizedSearchWords = normalizedTokens(normalizedSearchText);
  const normalizedSearchWordSet = new Set(normalizedSearchWords);
  return {
    id,
    kind,
    target,
    title,
    subtitle,
    symbolName,
    normalizedSearchText,
    normalizedSearchWords,
    normalizedSearchWordSet,
  };
}

/**
 * Test-only re-export of the entry constructor, mirroring the crate's public
 * `SettingsSearchEntry::new` (used by the oracle tests to build synthetic
 * entries with an explicit searchText). Not part of the production surface.
 */
export function makeEntryForTest(
  id: string,
  kind: SettingsSearchEntryKind,
  target: SettingsNavigationTarget,
  title: string,
  subtitle: string | null,
  symbolName: string,
  searchText: string,
): SettingsSearchEntry {
  return makeEntry(id, kind, target, title, subtitle, symbolName, searchText);
}

/** `sectionID(for:)` (Swift line 600; `index.rs:41-43`). */
export function sectionId(target: SettingsNavigationTarget): string {
  return `section:${target}`;
}

/** `settingID(for:idSuffix:)` (Swift line 601; `index.rs:46-48`). */
export function settingId(
  target: SettingsNavigationTarget,
  idSuffix: string,
): string {
  return `setting:${target}:${idSuffix}`;
}

/**
 * `setting(_:_:_:_:)` builder (Swift `SettingsSearchAliases.swift` lines
 * 184-199; `index.rs:54-77`). Composes the searchable string from the target
 * rawValue, the id suffix, the section `searchText`, the row `searchText`, and
 * the row aliases; subtitle is `target.title`, symbol is `target.symbolName`.
 */
export function settingEntry(
  target: SettingsNavigationTarget,
  idSuffix: string,
  title: string,
  searchText: string,
): SettingsSearchEntry {
  const composed = `${target} ${idSuffix} ${targetSearchText(target)} ${searchText} ${aliases(target, idSuffix)}`;
  return makeEntry(
    settingId(target, idSuffix),
    "setting",
    target,
    title,
    targetTitle(target),
    targetSymbolName(target),
    composed,
  );
}

// ----------------------------------------------------------------------------
// scorer (port of `index.rs:140-214`)
// ----------------------------------------------------------------------------

/**
 * Inner `matchScore(token:text:words:wordSet:)` (Swift lines 250-259;
 * `index.rs:186-214`). First-hit bonus: exact word 0, prefix 10, boundary 20,
 * contains 30, light-typo 50, subsequence-of-word 60, subsequence-of-text 80,
 * else `null`.
 */
export function matchScoreToken(
  token: string,
  text: string,
  words: readonly string[],
  wordSet: ReadonlySet<string>,
): number | null {
  if (wordSet.has(token)) {
    return 0;
  }
  if (words.some((word) => word.startsWith(token))) {
    return 10;
  }
  if (containsAtWordBoundary(token, text)) {
    return 20;
  }
  if (text.includes(token)) {
    return 30;
  }
  if (words.some((word) => isLightTypo(token, word))) {
    return 50;
  }
  if (words.some((word) => isSubsequence(token, word))) {
    return 60;
  }
  if (isSubsequence(token, text)) {
    return 80;
  }
  return null;
}

/**
 * `matchScore(entry:query:tokens:)` (Swift lines 225-248; `index.rs:140-181`).
 * `query` is the already-normalized, trimmed query string. Returns `null` when
 * any token fails to match; otherwise a score where lower sorts earlier.
 */
export function matchScore(
  entry: SettingsSearchEntry,
  query: string,
  tokens: readonly string[],
): number | null {
  let score = 0;
  for (const token of tokens) {
    const tokenScore = matchScoreToken(
      token,
      entry.normalizedSearchText,
      entry.normalizedSearchWords,
      entry.normalizedSearchWordSet,
    );
    if (tokenScore === null) {
      return null;
    }
    score += tokenScore;
  }

  const title = normalized(entry.title);
  if (title === query) {
    score -= 1000;
  }
  if (title.startsWith(query)) {
    score -= 800;
  }
  if (containsAtWordBoundary(query, title)) {
    score -= 700;
  }
  if (entry.normalizedSearchText.startsWith(query)) {
    score -= 600;
  }
  if (containsAtWordBoundary(query, entry.normalizedSearchText)) {
    score -= 500;
  }
  if (entry.normalizedSearchText.includes(query)) {
    score -= 400;
  }
  if (entry.kind === "section") {
    score += 25;
  }
  return score;
}

// ----------------------------------------------------------------------------
// corpus (port of `Sources/SettingsNavigation.swift:304-456` + scroll-speed)
// ----------------------------------------------------------------------------

/**
 * `sectionEntries` (Swift lines 304-314) — one entry per target in allCases
 * order. `null` subtitle; searchText = `"<rawValue> <target.searchText>
 * <sectionAliases>"`.
 */
export const sectionEntries: readonly SettingsSearchEntry[] =
  SETTINGS_NAVIGATION_TARGETS.map((target) =>
    makeEntry(
      sectionId(target),
      "section",
      target,
      targetTitle(target),
      null,
      targetSymbolName(target),
      `${target} ${targetSearchText(target)} ${sectionAliases(target)}`,
    ),
  );

// `settingEntries` rows (Swift lines 316-453) as `[target, idSuffix, title,
// rowSearchText]` tuples, transcribed verbatim and in Swift order (offset order
// is load-bearing for the score tiebreak).
type SettingRow = [SettingsNavigationTarget, string, string, string];

// The `automation:workspace-auto-naming` row is the only multi-string
// searchText — a `.joined(separator: " ")` of the 6 localized strings on Swift
// lines 411-416 (keyword blob + subtitleOn + subtitleOff + note + "Naming
// Agent" + "Automatic"). Concatenated English result:
const WORKSPACE_AUTO_NAMING_SEARCH_TEXT = [
  "automation.workspaceAutoNaming automation.autoNamingAgent workspace auto naming auto name ai naming names rename workspace rename tab title titles generated name agent summarizer summarize conversation",
  "Workspaces and tabs are named from agent conversations.",
  "Workspace and tab names are never generated.",
  "When enabled, cmux summarizes supported agent sessions into short workspace and tab names using each agent's own binary, refreshed as the topic shifts. Manual renames always win and stop auto-naming for that workspace or tab. Uses your agent account for the short summarization calls.",
  "Naming Agent",
  "Automatic",
].join(" ");

const SETTING_ROWS: readonly SettingRow[] = [
  ["account", "account", "Account", "sign in login team sync user profile"],
  ["app", "language", "Language", "locale translation japanese english restart"],
  ["app", "appearance", "Appearance", "theme light dark system"],
  ["app", "app-icon", "App Icon", "dock icon alternate"],
  ["app", "new-workspace-placement", "New Workspace Placement", "workspace order position"],
  [
    "app",
    "workspace-group-new-workspace-placement",
    "Group New Workspace Placement",
    "workspace group command n plus insert position after current top end",
  ],
  [
    "app",
    "fork-conversation-default",
    "Fork Conversation Default",
    "fork conversation default right left top bottom split tab workspace",
  ],
  [
    "app",
    "workspace-inherit-working-directory",
    "Inherit Workspace Working Directory",
    "workspace cwd directory current ghostty working-directory",
  ],
  ["app", "minimal-mode", "Minimal Mode", "presentation compact chrome"],
  [
    "app",
    "keep-workspace-open",
    "Keep Workspace Open When Closing Last Surface",
    "close last surface shortcut",
  ],
  ["app", "focus-pane-first-click", "Focus Pane on First Click", "mouse click focus"],
  [
    "app",
    "file-drops",
    "File Drops",
    "drag drop files finder path text terminal editor split preview shift",
  ],
  ["app", "preferred-editor", "Open Files With", "editor code zed subl cmd click file"],
  [
    "app",
    "supported-file-previews",
    "Open Supported Files in cmux",
    "cmd click file preview pdf image audio video quick look editor",
  ],
  [
    "app",
    "terminal-config",
    "Terminal Config",
    "ghostty config merged preview macos-option-as-alt option as alt left option right option alt key meta",
  ],
  ["app", "markdown-viewer", "Open Markdown in cmux Viewer", "md markdown viewer"],
  [
    "app",
    "markdown-font-size",
    "Markdown Viewer Font Size",
    "md markdown viewer font size points zoom scale text bigger smaller",
  ],
  [
    "app",
    "markdown-font-family",
    "Markdown Viewer Font",
    "markdown.fontFamily md markdown viewer font font-family family typeface system stack custom",
  ],
  [
    "app",
    "markdown-max-width",
    "Markdown Viewer Max Width",
    "markdown.maxWidth md markdown viewer width column reading line length pixels px",
  ],
  [
    "app",
    "file-editor-word-wrap",
    "File Editor Word Wrap",
    "fileEditor.wordWrap file editor word wrap soft wrap reflow lines text horizontal scroll preview",
  ],
  [
    "app",
    "imessage-mode",
    "iMessage Mode",
    "message messages imessage chat prompt prompts submitted message send agent workspace reorder move top",
  ],
  ["app", "reorder-notification", "Reorder on Notification", "workspace notification order"],
  ["app", "dock-badge", "Dock Badge", "unread count app icon"],
  ["app", "menu-bar-only", "Menu Bar Only", "dock icon cmd tab"],
  ["app", "show-menu-bar", "Show in Menu Bar", "menu extra status item"],
  ["app", "unread-pane-ring", "Unread Pane Ring", "notification blue ring pane"],
  ["app", "pane-flash", "Pane Flash", "notification flash highlight"],
  ["app", "desktop-notifications", "Desktop Notifications", "permission alerts test notification"],
  ["app", "notification-sound", "Notification Sound", "custom sound alert audio"],
  ["app", "notification-command", "Notification Command", "shell command environment variables"],
  ["app", "telemetry", "Send anonymous telemetry", "analytics crash usage"],
  [
    "app",
    "default-terminal",
    "Default Terminal",
    "ssh links command tool unix executable launch services handler registration system default",
  ],
  ["app", "warn-before-quit", "Warn Before Quit", "cmd q confirmation confirmQuit"],
  ["app", "warn-before-closing-tab", "Warn Before Closing Tab", "cmd w close tab confirmation"],
  [
    "app",
    "warn-before-closing-tab-x-button",
    "Warn Before Tab Close Button",
    "x button close tab confirmation",
  ],
  ["app", "hide-tab-close-button", "Hide Tab Close Button", "hide x button close tab"],
  [
    "app",
    "rename-selects-name",
    "Rename Selects Existing Name",
    "command palette rename text selection",
  ],
  [
    "app",
    "palette-search-all",
    "Command Palette Searches All Surfaces",
    "cmd p search terminal browser markdown",
  ],
  [
    "app",
    "canvas-pane-gap",
    "Canvas Pane Gap",
    "canvas.paneGap canvas pane gap spacing freeform layout panes snapping tidy distribute align",
  ],
  [
    "app",
    "canvas-snapping",
    "Canvas Snapping",
    "canvas.snappingEnabled canvas snap snapping enabled edges drag resize align panes freeform layout",
  ],
  ["terminal", "scrollbar", "Show Terminal Scroll Bar", "terminal shell scrollback"],
  [
    "terminal",
    "copy-on-select",
    "Copy on Selection",
    "terminal.copyOnSelect clipboard selection mouse double click triple click",
  ],
  [
    "terminal",
    "tab-bar-font-size",
    "Tab Bar Font Size",
    "font size text scale terminal browser pane tab title surface-tab-bar-font-size",
  ],
  [
    "terminal",
    "agent-auto-resume",
    "Resume Agent Sessions on Reopen",
    "terminal.autoResumeAgentSessions auto resume restore reopen relaunch quit sessions agents claude code codex opencode rovo dev rovodev toggle",
  ],
  [
    "terminal",
    "agent-hibernation",
    "Agent Hibernation",
    "terminal.agentHibernation idle hibernate suspend background agents claude code codex opencode live terminals",
  ],
  [
    "terminal",
    "renderer-realization",
    "Reclaim Offscreen Terminal Memory",
    "terminal.rendererRealization renderer reclaim offscreen memory iosurface gpu idle warm release background terminals",
  ],
  [
    "terminal",
    "resume-commands",
    "Resume Commands",
    "surface resume command approvals prefixes auto restore prompt manual tmux hibernation",
  ],
  [
    "textBox",
    "show-textbox-new-terminals",
    "Show TextBox on New Terminals",
    "terminal.showTextBoxOnNewTerminals textbox text box rich input prompt default new workspace split tab beta",
  ],
  [
    "textBox",
    "focus-textbox-new-terminals",
    "Focus TextBox on New Terminals",
    "terminal.focusTextBoxOnNewTerminals textbox text box rich input prompt default new workspace split tab beta",
  ],
  [
    "textBox",
    "textbox-max-lines",
    "TextBox Max Lines",
    "terminal.textBoxMaxLines terminal textbox text box rich input prompt max height lines grow scroll beta",
  ],
  ["sidebarAppearance", "match-terminal", "Match Terminal Background", "sidebar material transparency"],
  [
    "sidebarAppearance",
    "font-size",
    "Sidebar Font Size",
    "font size text scale workspace title badge metadata shortcut hint sidebar-font-size",
  ],
  ["sidebarAppearance", "hide-sidebar-details", "Hide All Sidebar Details", "workspace sidebar compact"],
  [
    "sidebarAppearance",
    "wrap-workspace-titles",
    "Wrap Workspace Titles in Sidebar",
    "workspace title wrap multiline pr pull request",
  ],
  [
    "sidebarAppearance",
    "show-workspace-description",
    "Show Workspace Description in Sidebar",
    "workspace description notes markdown",
  ],
  ["sidebarAppearance", "sidebar-branch-layout", "Sidebar Branch Layout", "branch directory vertical inline"],
  [
    "sidebarAppearance",
    "stack-branch-directory",
    "Stack Branch and Directory",
    "branch directory cwd path stack two rows separate lines",
  ],
  [
    "sidebarAppearance",
    "path-last-segment-only",
    "Truncate Path From Start",
    "cwd path directory truncate last segment basename viewport",
  ],
  [
    "sidebarAppearance",
    "show-notification-message",
    "Show Notification Message in Sidebar",
    "workspace latest notification",
  ],
  ["sidebarAppearance", "show-branch-directory", "Show Branch + Directory in Sidebar", "git cwd path"],
  ["sidebarAppearance", "show-pull-requests", "Show Pull Requests in Sidebar", "review pr mr link"],
  [
    "sidebarAppearance",
    "watch-git-status",
    "Watch Git Status in Sidebar",
    "git status branch watcher index lock",
  ],
  [
    "sidebarAppearance",
    "make-pr-clickable",
    "Make Sidebar PR Clickable",
    "pull requests pull request pr mr review clickable links select workspace row",
  ],
  ["sidebarAppearance", "open-pr-links", "Open Sidebar PR Links in cmux Browser", "pull request link browser"],
  ["sidebarAppearance", "open-port-links", "Open Sidebar Port Links in cmux Browser", "port link browser"],
  ["sidebarAppearance", "show-ssh", "Show SSH in Sidebar", "remote target"],
  ["sidebarAppearance", "show-ports", "Show Listening Ports in Sidebar", "localhost port"],
  ["sidebarAppearance", "show-log", "Show Latest Log in Sidebar", "status message"],
  ["sidebarAppearance", "show-progress", "Show Progress in Sidebar", "progress bar"],
  ["sidebarAppearance", "show-metadata", "Show Custom Metadata in Sidebar", "report meta status block"],
  [
    "sidebarAppearance",
    "right-max-width",
    "Dock Max Width",
    "dock right sidebar max width terminal reservation cap logs lazygit",
  ],
  [
    "customSidebars",
    "enabled",
    "Show Custom Sidebars",
    "custom sidebars enable show vibe swift json interpreted picker",
  ],
  [
    "customSidebars",
    "renderer",
    "Renderer",
    "renderer in-process in app remote worker isolated process hover focus typing input",
  ],
  ["betaFeatures", "feed", "Feed", "feed right sidebar agent decisions permissions questions"],
  ["betaFeatures", "dock", "Dock", "dock right sidebar terminal controls tui"],
  ["automation", "socket-mode", "Socket Control Mode", "unix socket api access password auth"],
  ["automation", "socket-password", "Socket Password", "socket auth credential"],
  ["automation", "claude-code", "Claude Code Integration", "agent hooks notifications"],
  ["automation", "claude-path", "Claude Binary Path", "custom claude executable"],
  ["automation", "workspace-auto-naming", "Workspace Auto-Naming", WORKSPACE_AUTO_NAMING_SEARCH_TEXT],
  [
    "automation",
    "ripgrep-path",
    "Ripgrep Binary Path",
    "custom ripgrep rg executable find search nix",
  ],
  [
    "automation",
    "subagent-notifications",
    "Suppress Subagent Notifications",
    "nested child agent codex claude hooks notifications",
  ],
  ["automation", "cursor", "Cursor Integration", "agent hooks notifications"],
  ["automation", "gemini", "Gemini CLI Integration", "agent hooks notifications"],
  ["automation", "kiro", "Kiro CLI Integration", "agent hooks notifications"],
  [
    "automation",
    "kiro-notification-level",
    "Kiro Notification Level",
    "agent hooks notifications verbosity tool events",
  ],
  ["automation", "port-base", "Port Base", "CMUX_PORT start"],
  ["automation", "port-range", "Port Range Size", "CMUX_PORT_END workspace ports"],
  [
    "browser",
    "search-engine",
    "Default Search Engine",
    "address bar query google duckduckgo bing kagi brave startpage perplexity exa yahoo ecosia qwant mojeek wikipedia github baidu yandex custom search provider",
  ],
  ["browser", "enable-browser", "Enable cmux Browser", "webview tabs links"],
  ["browser", "search-suggestions", "Show Search Suggestions", "browser address bar suggestions"],
  ["browser", "theme", "Browser Theme", "web appearance light dark system"],
  [
    "browser",
    "hidden-webview-discard",
    "Discard Hidden Browser WebViews",
    "memory hidden tabs webview discard unload",
  ],
  [
    "browser",
    "hidden-webview-discard-delay",
    "Hidden WebView Discard Delay",
    "memory hidden tabs delay seconds discard",
  ],
  ["browser", "terminal-links", "Open Terminal Links in cmux Browser", "click links browser"],
  ["browser", "intercept-open", "Intercept open http(s) in Terminal", "open command urls"],
  ["browser", "host-whitelist", "Hosts to Open in Embedded Browser", "hosts wildcard terminal links"],
  ["browser", "external-patterns", "URLs to Always Open Externally", "regex url rules default browser"],
  ["browser", "http-allowlist", "HTTP Hosts Allowed in Embedded Browser", "localhost non https warning"],
  ["browserImport", "import-data", "Import Browser Data", "bookmarks history cookies profiles"],
  ["browserImport", "import-hint", "Show import hint on blank browser tabs", "blank tab browser import"],
  ["browser", "react-grab", "React Grab Version", "npm react grab toolbar"],
  ["browser", "history", "Browsing History", "clear visited suggestions"],
  ["globalHotkey", "enable-hotkey", "Enable System-Wide Hotkey", "global shortcut show hide windows"],
  ["globalHotkey", "shortcut", "Global Hotkey", "keyboard recorder command option control"],
  ["keyboardShortcuts", "shortcut-chords", "Shortcut Chords", "tmux multi step keybindings"],
  [
    "keyboardShortcuts",
    "reset-defaults",
    "Reset Default Shortcuts",
    "restore built in builtin defaults keybindings hotkeys chords commands",
  ],
  ["keyboardShortcuts", "shortcuts", "Keyboard Shortcuts", "keybindings commands"],
  ["workspaceColors", "indicator", "Workspace Color Indicator", "tab color indicator"],
  ["workspaceColors", "selection", "Selection Highlight", "selected workspace background"],
  ["workspaceColors", "badge", "Notification Badge", "unread notification color"],
  ["workspaceColors", "palette", "Reset Palette", "named colors palette"],
  ["settingsJSON", "open-file", "Open cmux.json", "config json file editor dotfiles"],
  ["settingsJSON", "documentation", "Documentation", "cmux json schema reference docs"],
  ["reset", "reset-all", "Reset All Settings", "restore defaults"],
  // `+ terminalScrollSpeedSettingEntries` (Swift line 454;
  // `Sources/SettingsNavigation+TerminalScrollSpeed.swift:4-13`).
  ["terminal", "scroll-speed", "Scroll Speed", "terminal scroll speed multiplier wheel trackpad sensitivity"],
];

/** `settingEntries` (Swift lines 316-454). */
export const settingEntries: readonly SettingsSearchEntry[] = SETTING_ROWS.map(
  ([target, idSuffix, title, searchText]) =>
    settingEntry(target, idSuffix, title, searchText),
);

/** `allEntries` (Swift line 456) — sections followed by settings. */
export const allEntries: readonly SettingsSearchEntry[] = [
  ...sectionEntries,
  ...settingEntries,
];

// ----------------------------------------------------------------------------
// producer (port of `entries(matching:)`, Swift lines 574-590)
// ----------------------------------------------------------------------------

/**
 * `entries(matching:)` (Swift lines 574-590). Empty query / stop-words-only ->
 * the 16 section entries (allCases order). Otherwise scores every entry against
 * the normalized+trimmed query, drops non-matches, and sorts ascending by score
 * with ascending original offset as the stable tiebreak (lower score = more
 * relevant).
 */
export function settingsEntriesMatching(
  query: string,
): SettingsSearchEntry[] {
  const tokens = normalizedQueryTokens(query);
  if (tokens.length === 0) {
    return [...sectionEntries];
  }
  const normalizedQuery = normalized(query).trim();
  const scored: { entry: SettingsSearchEntry; score: number; offset: number }[] =
    [];
  allEntries.forEach((entry, offset) => {
    const score = matchScore(entry, normalizedQuery, tokens);
    if (score !== null) {
      scored.push({ entry, score, offset });
    }
  });
  scored.sort((lhs, rhs) =>
    lhs.score !== rhs.score ? lhs.score - rhs.score : lhs.offset - rhs.offset,
  );
  return scored.map((item) => item.entry);
}
