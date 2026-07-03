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
  return body.trim();
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
