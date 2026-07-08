// Port of the canonical shortcut display formatter:
// `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/ShortcutDisplayFormatter.swift:1-165`
// with input value types from
// `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/StoredShortcut.swift:10-31` and
// `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/ShortcutStroke.swift:11-37`.
// The thin UI wrapper mirrored by `shortcutDisplayString` is
// `Packages/macOS/CmuxSettingsUI/Sources/CmuxSettingsUI/Rows/ShortcutDisplayString.swift`.
//
// SANCTIONED DIVERGENCE (localization): Swift routes the fixed label set
// ("None", "Tab", "Space", media keys) through `String(localized:)`. The web
// shell is English-only today, so the English `defaultValue` strings are
// inlined verbatim, consistent with the other ported web modules.

/**
 * One keystroke in a (possibly chorded) shortcut. Mirrors Swift
 * `ShortcutStroke` (ShortcutStroke.swift:11-37) minus `keyCode`, which is a
 * macOS virtual-key re-matching aid and never affects display.
 *
 * Modifier flags default to absent/false, mirroring the Swift init defaults.
 */
export interface ShortcutStroke {
  key: string;
  command?: boolean;
  shift?: boolean;
  option?: boolean;
  control?: boolean;
}

/**
 * A shortcut binding as stored on disk: one or two strokes. Mirrors Swift
 * `StoredShortcut` (StoredShortcut.swift:10-31). `second` absent/null means a
 * single-stroke binding.
 */
export interface StoredShortcut {
  first: ShortcutStroke;
  second?: ShortcutStroke | null;
}

/**
 * The explicit "no shortcut" marker. Mirrors `StoredShortcut.unbound`
 * (StoredShortcut.swift:18-20): an empty first key and no second stroke.
 */
// Frozen: parse fns return this shared instance by reference, so a caller
// mutating its result would silently corrupt every later "unbound" parse.
export const UNBOUND_SHORTCUT: StoredShortcut = Object.freeze({
  first: Object.freeze({
    key: "",
    command: false,
    shift: false,
    option: false,
    control: false,
  }),
});

/**
 * True when this binding is the explicit "no shortcut" marker. Mirrors
 * `StoredShortcut.isUnbound` (StoredShortcut.swift:28).
 *
 * Note: an empty `first.key` WITH a second stroke is NOT unbound — it falls
 * through to normal rendering, where the empty key uppercases to "" and only
 * the modifiers show.
 */
export function isUnbound(shortcut: StoredShortcut): boolean {
  return shortcut.first.key === "" && shortcut.second == null;
}

/**
 * The range label shown for numbered workspace/surface shortcut families.
 * Mirrors `numberedDigitRangeHint` (ShortcutDisplayFormatter.swift:13).
 * U+2026 HORIZONTAL ELLIPSIS, not three dots.
 */
export const NUMBERED_DIGIT_RANGE_HINT = "1…9";

/**
 * Parses an integer with Swift `Int(String)` semantics: an optional single
 * leading `+`/`-`, then one or more ASCII digits, and nothing else — no
 * whitespace, no underscores, no non-ASCII digits. Returns `null` otherwise.
 *
 * Deliberately NOT `parseInt` (accepts trailing junk and whitespace) nor bare
 * `Number` (accepts whitespace, `"0x5"`, empty string).
 *
 * Overflow: Swift returns nil past Int64 range. This port returns `null` once
 * the digit run (after stripping leading zeros) exceeds 15 digits — every
 * magnitude ≥ 10^15 is equally out of range for the 1…9 / 1…20 comparisons
 * this module performs, so the observable behavior is identical.
 */
export function swiftParseInt(s: string): number | null {
  const m = /^([+-]?)([0-9]+)$/.exec(s);
  if (m === null) {
    return null;
  }
  const digits = m[2].replace(/^0+(?=[0-9])/, "");
  if (digits.length > 15) {
    return null;
  }
  const value = Number(m[1] === "-" ? `-${digits}` : digits);
  return Number.isSafeInteger(value) ? value : null;
}

/**
 * Whether a key token is a valid numbered shortcut placeholder digit.
 * Mirrors `isNumberedDigitKey` (ShortcutDisplayFormatter.swift:152-155):
 * Swift `Int(key)` parse, then range check 1...9. So `"05"` and `"+3"` are
 * numbered digits (Swift Int parses them), while `"0"`, `"10"`, `" 1"`, and
 * non-ASCII digits are not.
 */
export function isNumberedDigitKey(key: string): boolean {
  const digit = swiftParseInt(key);
  return digit !== null && digit >= 1 && digit <= 9;
}

/**
 * Formats modifier booleans in cmux's standard Control, Option, Shift,
 * Command order regardless of input. Mirrors `modifierDisplayString`
 * (ShortcutDisplayFormatter.swift:85-97).
 */
export function modifierDisplayString(stroke: ShortcutStroke): string {
  let result = "";
  if (stroke.control) result += "⌃";
  if (stroke.option) result += "⌥";
  if (stroke.shift) result += "⇧";
  if (stroke.command) result += "⌘";
  return result;
}

/**
 * F-key rule. Mirrors `functionKeyDisplayString(for:)`
 * (ShortcutDisplayFormatter.swift:157-164): lowercase `"f"` prefix + Swift
 * Int parse of the remainder in 1...20 → `F<n>`. `"F5"` does not match the
 * lowercase prefix and falls to the uppercase default (visually "F5" anyway);
 * `"f05"` → "F5" and `"f+1"` → "F1" because Swift Int accepts them.
 */
function functionKeyDisplayString(key: string): string | null {
  if (!key.startsWith("f")) {
    return null;
  }
  const number = swiftParseInt(key.slice(1));
  if (number === null || number < 1 || number > 20) {
    return null;
  }
  return `F${number}`;
}

/**
 * Formats a stored shortcut key token. Mirrors `keyDisplayString`
 * (ShortcutDisplayFormatter.swift:116-146).
 *
 * The default arm uses `toUpperCase()` (locale-independent full Unicode
 * uppercase, matching Swift `String.uppercased()`: both map "ß" → "SS" and
 * leave symbols like "←" unchanged). Deliberately NOT `toLocaleUpperCase`.
 */
export function keyDisplayString(key: string): string {
  switch (key) {
    case "\t":
      return "Tab";
    case "space":
      return "Space";
    case "\r":
      return "↩";
    case "media.brightnessDown":
      return "Brightness Down";
    case "media.brightnessUp":
      return "Brightness Up";
    case "media.mute":
      return "Mute";
    case "media.next":
      return "Next Track";
    case "media.playPause":
      return "Play/Pause";
    case "media.previous":
      return "Previous Track";
    case "media.volumeDown":
      return "Volume Down";
    case "media.volumeUp":
      return "Volume Up";
    default: {
      const fKey = functionKeyDisplayString(key);
      return fKey !== null ? fKey : key.toUpperCase();
    }
  }
}

/**
 * Formats a single shortcut stroke: modifier glyphs followed by the key
 * label. Mirrors `displayString(_ stroke:)` / `strokeDisplayString`
 * (ShortcutDisplayFormatter.swift:47-75).
 */
export function strokeDisplayString(stroke: ShortcutStroke): string {
  return modifierDisplayString(stroke) + keyDisplayString(stroke.key);
}

/**
 * Formats a stored shortcut, optionally treating digits 1...9 as the
 * `NUMBERED_DIGIT_RANGE_HINT` placeholder. Mirrors
 * `displayString(_ shortcut:numbered:)` (ShortcutDisplayFormatter.swift:21-41)
 * — exact branch order:
 *
 * 1. Unbound → "None".
 * 2. Numbered + chord whose SECOND key is a 1…9 digit → first stroke, space,
 *    second stroke's modifiers + "1…9".
 * 3. Numbered + single stroke whose key is a 1…9 digit → modifiers + "1…9".
 * 4. Otherwise (including a numbered chord whose second key is NOT a digit):
 *    plain chord "first second" or plain single stroke.
 */
export function shortcutDisplayString(shortcut: StoredShortcut, numbered = false): string {
  if (isUnbound(shortcut)) {
    return "None";
  }
  const second = shortcut.second ?? null;
  if (numbered) {
    if (second !== null) {
      if (isNumberedDigitKey(second.key)) {
        return (
          strokeDisplayString(shortcut.first) +
          " " +
          modifierDisplayString(second) +
          NUMBERED_DIGIT_RANGE_HINT
        );
      }
    } else if (isNumberedDigitKey(shortcut.first.key)) {
      return modifierDisplayString(shortcut.first) + NUMBERED_DIGIT_RANGE_HINT;
    }
  }
  if (second !== null) {
    return `${strokeDisplayString(shortcut.first)} ${strokeDisplayString(second)}`;
  }
  return strokeDisplayString(shortcut.first);
}
