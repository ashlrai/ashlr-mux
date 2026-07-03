// Port of `crates/cmux-command-palette/src/list_scope.rs` +
// `overlay_promotion.rs`, plus the query→scope derivation and the
// visible-results reset transition that live in the macOS host
// (`Sources/ContentView.swift`: `commandPaletteListScope(for:)`,
// `commandPaletteShouldResetVisibleResultsForQueryTransition`).
//
// The fuzzy MATCHER stays in Rust; this module only carries the pure
// scope sub-policies.

/**
 * Which list the palette is showing: the `>`-prefixed command list or the
 * workspace/surface switcher.
 *
 * Values mirror the Swift `CommandPaletteListScope` raw values
 * (`commands` / `switcher`), which the host uses as list identity.
 */
export type CommandPaletteListScope = "commands" | "switcher";

/**
 * Prefix that switches the palette into the command list.
 *
 * Swift: `commandPaletteCommandsPrefix = ">"`.
 */
export const COMMAND_PALETTE_COMMANDS_PREFIX = ">";

/**
 * Derives the list scope from the raw query.
 *
 * Swift `commandPaletteListScope(for:)`: a `>`-prefixed query shows the
 * command list; anything else shows the switcher.
 */
export function listScope(query: string): CommandPaletteListScope {
  return query.startsWith(COMMAND_PALETTE_COMMANDS_PREFIX) ? "commands" : "switcher";
}

/**
 * Trims exactly the code points in Swift's `CharacterSet.whitespacesAndNewlines`
 * from both ends of `s`.
 *
 * That set is `.whitespaces` (Unicode General Category Zs plus U+0009 TAB)
 * unioned with `.newlines` (U+000A LF, U+000B VT, U+000C FF, U+000D CR,
 * U+0085 NEL, U+2028 LS, U+2029 PS).
 *
 * This is deliberately NOT `String.prototype.trim()`, whose whitespace set
 * diverges from Swift's: JS `.trim()` strips U+FEFF (ZWNBSP/BOM) — which is
 * Unicode category Cf, not Zs, so Swift keeps it — and does NOT strip U+0085
 * (NEL), which Swift trims. Using `.trim()` here would drift from the host.
 */
export function trimWhitespaceAndNewlines(s: string): string {
  // Swift's whitespacesAndNewlines set. Note U+FEFF is intentionally absent.
  const cls =
    "\\u0009\\u000A\\u000B\\u000C\\u000D\\u0020\\u0085\\u00A0\\u1680" +
    "\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000";
  return s.replace(new RegExp(`^[${cls}]+|[${cls}]+$`, "gu"), "");
}

/**
 * The query the matcher should search, after removing scope framing.
 *
 * Swift `commandPaletteQueryForMatching(query:scope:)`: strip the leading
 * `>` in the commands scope, then trim surrounding whitespace/newlines in
 * either scope. The trimmed remainder is what the (Rust) matcher consumes.
 */
export function queryForMatching(query: string): string {
  const scope = listScope(query);
  const body =
    scope === "commands" ? query.slice(COMMAND_PALETTE_COMMANDS_PREFIX.length) : query;
  return trimWhitespaceAndNewlines(body);
}

/**
 * Whether the palette overlay should be promoted above its sibling overlay
 * views: exactly on the hidden→visible transition, so an already-visible
 * palette is not reshuffled on every state update.
 *
 * Port of `CommandPaletteOverlayPromotionPolicy::should_promote`.
 */
export function shouldPromoteOverlay(previouslyVisible: boolean, isVisible: boolean): boolean {
  return isVisible && !previouslyVisible;
}

/**
 * Whether the visible result list should be reset when the query changes.
 *
 * Swift `commandPaletteShouldResetVisibleResultsForQueryTransition`: reset
 * only when results are currently shown AND the scope flipped between the
 * old and new query (e.g. typing/removing the `>` prefix).
 */
export function shouldResetVisibleResults(
  oldQuery: string,
  newQuery: string,
  hasVisibleResults: boolean,
): boolean {
  return hasVisibleResults && listScope(oldQuery) !== listScope(newQuery);
}
