// Port of the canonical shortcut config-string parse path, projecting the
// generated `ShortcutBinding` config value into the `StoredShortcut` consumed
// by the ported display formatter (`shortcutFormat.ts`). Oracle:
// - `Sources/KeyboardShortcutSettings.swift:2444-2513` — `parseConfigKeyToken`
// - `Sources/KeyboardShortcutSettings.swift:2385-2423` — `ShortcutStroke.parseConfig`
// - `Sources/KeyboardShortcutSettings.swift:2546-2552` — `isUnboundConfigToken`
// - `Sources/KeyboardShortcutSettings.swift:2517-2536` — `StoredShortcut.parseConfig`
// - `Sources/KeyboardShortcutSettingsFileStore.swift:994-1027` —
//   `parseShortcutBindingValue` (string/array/NSNull arms)
// - `Sources/KeyboardShortcutSettings.swift:582-604` + `623-631` — per-action
//   numbered/bare metadata and `displayedShortcutString`.
//
// DOCUMENTED OMISSIONS:
// - The nested-object recorder form ({ first: { key, ... }, second? };
//   `KeyboardShortcutSettingsFileStore.swift:1037-1066`) is unrepresentable in
//   the generated TS `ShortcutBinding` type (`string | string[]`) — skipped.
// - `action.normalizedSettingsFileShortcut` post-normalization
//   (`KeyboardShortcutSettings.swift:695-720`, applied at FileStore:1026)
//   needs the live conflict-checking store and only rewrites numbered digit
//   keys to "1", which is display-invisible under the numbered=true
//   formatter — skipped.

import type { ShortcutBinding } from "@cmux/core-types";

import {
  UNBOUND_SHORTCUT,
  shortcutDisplayString,
  swiftParseInt,
  type ShortcutStroke,
  type StoredShortcut,
} from "./shortcutFormat";

/**
 * Actions whose digit key renders as the "1…9" range placeholder. Mirrors
 * `Action.usesNumberedDigitMatching` (KeyboardShortcutSettings.swift:582-589).
 * Action ids are the Swift enum case names (rawValue == case name).
 */
export const NUMBERED_DIGIT_ACTIONS: ReadonlySet<string> = new Set([
  "selectSurfaceByNumber",
  "selectWorkspaceByNumber",
]);

/**
 * Actions whose first stroke may carry no modifier. Mirrors
 * `Action.allowsBareFirstStroke` (KeyboardShortcutSettings.swift:591-604).
 */
export const BARE_FIRST_STROKE_ACTIONS: ReadonlySet<string> = new Set([
  "diffViewerScrollDown",
  "diffViewerScrollUp",
  "diffViewerScrollToBottom",
  "diffViewerScrollToTop",
  "diffViewerOpenFileSearch",
  "fileExplorerOpenSelection",
  "fileExplorerOpenSelectionFinderAlias",
]);

/**
 * Parses one config key token into a stored-shortcut key. Mirrors
 * `parseConfigKeyToken` (KeyboardShortcutSettings.swift:2444-2513): trim, then
 * an exact lowercased alias table, then the f1…f20 rule, then any single
 * character. Returns `null` for unrecognized tokens.
 *
 * The single-character arm counts code points (Swift counts grapheme
 * clusters; no single-token config key differs between the two in practice).
 */
export function parseConfigKeyToken(rawValue: string): string | null {
  const trimmed = rawValue.trim();
  if (trimmed === "") {
    // A literal single space means the space key; other all-whitespace
    // tokens are invalid (Swift 2446-2448).
    return rawValue === " " ? "space" : null;
  }

  const lowered = trimmed.toLowerCase();
  switch (lowered) {
    case "left":
    case "arrowleft":
    case "leftarrow":
    case "←":
      return "←";
    case "right":
    case "arrowright":
    case "rightarrow":
    case "→":
      return "→";
    case "up":
    case "arrowup":
    case "uparrow":
    case "↑":
      return "↑";
    case "down":
    case "arrowdown":
    case "downarrow":
    case "↓":
      return "↓";
    case "tab":
      return "\t";
    case "return":
    case "enter":
    case "↩":
      return "\r";
    case "space":
    case "spacebar":
    case "<space>":
      return "space";
    case "comma":
      return ",";
    case "period":
    case "dot":
      return ".";
    case "slash":
      return "/";
    case "backslash":
      return "\\";
    case "semicolon":
      return ";";
    case "quote":
    case "apostrophe":
      return "'";
    case "backtick":
    case "grave":
      return "`";
    case "minus":
    case "hyphen":
      return "-";
    case "plus":
    case "equals":
      return "=";
    case "leftbracket":
    case "openbracket":
      return "[";
    case "rightbracket":
    case "closebracket":
      return "]";
    case "volumeup":
    case "mediavolumeup":
    case "media.volumeup":
      return "media.volumeUp";
    case "volumedown":
    case "mediavolumedown":
    case "media.volumedown":
      return "media.volumeDown";
    case "brightnessup":
    case "mediabrightnessup":
    case "media.brightnessup":
      return "media.brightnessUp";
    case "brightnessdown":
    case "mediabrightnessdown":
    case "media.brightnessdown":
      return "media.brightnessDown";
    case "mute":
    case "mediamute":
    case "media.mute":
      return "media.mute";
    case "playpause":
    case "mediaplaypause":
    case "media.playpause":
      return "media.playPause";
    case "nexttrack":
    case "medianext":
    case "media.next":
    case "media.nexttrack":
      return "media.next";
    case "previoustrack":
    case "mediaprevious":
    case "media.previous":
    case "media.previoustrack":
      return "media.previous";
    default: {
      if (lowered.startsWith("f")) {
        // Swift Int parse accepts "+5"/"05", so "f05" normalizes to "f5".
        const number = swiftParseInt(lowered.slice(1));
        if (number !== null && number >= 1 && number <= 20) {
          return `f${number}`;
        }
      }
      return Array.from(lowered).length === 1 ? lowered : null;
    }
  }
}

/**
 * Parses one "mod+mod+key" config stroke. Mirrors `ShortcutStroke.parseConfig`
 * (KeyboardShortcutSettings.swift:2385-2423). JS `split("+")` keeps empty
 * pieces, matching Swift `omittingEmptySubsequences: false` — so "cmd+" fails
 * the non-empty-last-RAW-part guard while "cmd+ " reaches
 * `parseConfigKeyToken(" ")` and yields the space key. Modifier tokens are
 * trimmed + lowercased; the key token receives the RAW last part.
 */
export function parseConfigStroke(rawValue: string): ShortcutStroke | null {
  if (rawValue === "") {
    return null;
  }

  const rawParts = rawValue.split("+");
  const lastRawPart = rawParts[rawParts.length - 1];
  if (lastRawPart === "") {
    return null;
  }

  let command = false;
  let shift = false;
  let option = false;
  let control = false;

  for (const modifier of rawParts.slice(0, -1)) {
    switch (modifier.trim().toLowerCase()) {
      case "cmd":
      case "command":
      case "⌘":
        command = true;
        break;
      case "shift":
      case "⇧":
        shift = true;
        break;
      case "opt":
      case "option":
      case "alt":
      case "⌥":
        option = true;
        break;
      case "ctrl":
      case "control":
      case "ctl":
      case "⌃":
        control = true;
        break;
      default:
        return null;
    }
  }

  const key = parseConfigKeyToken(lastRawPart);
  if (key === null) {
    return null;
  }
  return { key, command, shift, option, control };
}

/**
 * True for the explicit "no shortcut" config sentinels. Mirrors
 * `isUnboundConfigToken` (KeyboardShortcutSettings.swift:2546-2552). A single
 * literal space is NOT an unbind token — it is the space key.
 */
export function isUnboundConfigToken(rawValue: string): boolean {
  if (rawValue === "") {
    return true;
  }
  if (rawValue === " ") {
    return false;
  }
  const normalized = rawValue.trim().toLowerCase();
  if (normalized === "") {
    return true;
  }
  return (
    normalized === "none" ||
    normalized === "clear" ||
    normalized === "unbound" ||
    normalized === "disabled"
  );
}

/**
 * Parses one or two config strokes into a stored shortcut. Mirrors
 * `StoredShortcut.parseConfig(strokes:allowBareFirstStroke:)`
 * (KeyboardShortcutSettings.swift:2524-2536). A malformed chord half
 * invalidates the whole binding; the first stroke must carry a modifier
 * unless the action allows a bare first stroke or the key is "space".
 */
export function parseStoredShortcut(
  strokes: string[],
  allowBareFirstStroke = false,
): StoredShortcut | null {
  if (strokes.length === 0 || strokes.length > 2) {
    return null;
  }
  if (strokes.length === 1 && isUnboundConfigToken(strokes[0])) {
    return UNBOUND_SHORTCUT;
  }

  const parsed: ShortcutStroke[] = [];
  for (const stroke of strokes) {
    const parsedStroke = parseConfigStroke(stroke);
    if (parsedStroke === null) {
      return null;
    }
    parsed.push(parsedStroke);
  }

  const first = parsed[0];
  const hasModifier = Boolean(first.command || first.shift || first.option || first.control);
  if (!allowBareFirstStroke && !hasModifier && first.key !== "space") {
    return null;
  }
  return parsed.length === 2 ? { first, second: parsed[1] } : { first };
}

/**
 * Parses a config-file `ShortcutBinding` value. Mirrors the string/array/
 * NSNull arms of `parseShortcutBindingValue`
 * (KeyboardShortcutSettingsFileStore.swift:994-1027); the string arm mirrors
 * `StoredShortcut.parseConfig(_:)` (KeyboardShortcutSettings.swift:2517-2522).
 * `null`/absent is the canonical NSNull explicit unbind.
 */
export function parseShortcutBinding(
  binding: ShortcutBinding | null | undefined,
  allowBareFirstStroke = false,
): StoredShortcut | null {
  if (binding == null) {
    return UNBOUND_SHORTCUT;
  }
  if (typeof binding === "string") {
    return isUnboundConfigToken(binding)
      ? UNBOUND_SHORTCUT
      : parseStoredShortcut([binding], allowBareFirstStroke);
  }
  return binding.length === 0
    ? UNBOUND_SHORTCUT
    : parseStoredShortcut(binding, allowBareFirstStroke);
}

/**
 * Display string for one action's config binding. Mirrors
 * `displayedShortcutString` (KeyboardShortcutSettings.swift:623-631) with the
 * per-action numbered/bare metadata applied.
 *
 * DOCUMENTED INTERIM on parse failure: canonical silently discards an invalid
 * config value and falls back to the built-in default binding, but the web
 * port has no defaults catalog yet, so the raw config text (array joined with
 * a space) is the honest display until it lands.
 */
export function shortcutBindingDisplayString(
  actionId: string,
  binding: ShortcutBinding | null | undefined,
): string {
  const parsed = parseShortcutBinding(binding, BARE_FIRST_STROKE_ACTIONS.has(actionId));
  if (parsed !== null) {
    return shortcutDisplayString(parsed, NUMBERED_DIGIT_ACTIONS.has(actionId));
  }
  if (binding == null) {
    // Unreachable (null parses to UNBOUND_SHORTCUT); keeps the fallback total.
    return "None";
  }
  return Array.isArray(binding) ? binding.join(" ") : binding;
}
