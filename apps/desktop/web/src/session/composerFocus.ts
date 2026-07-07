/**
 * Composer auto-focus for the agent pane.
 *
 * The reused canonical chat composer (`webviews/agent-session`) already sends on
 * Enter and inserts a newline on Shift+Enter — but only while the ProseMirror
 * editor itself holds focus. In the desktop shell the agent surface is never
 * unmounted; a pane toggles terminal⇄agent by flipping `display` (see the flat
 * portal in `Workspace.tsx`). Calling `.focus()` on a `display:none` node is a
 * no-op, so we cannot focus once at mount — we must (re)focus each time the
 * surface becomes visible.
 *
 * This module holds the pure focus *decision* so it is unit-testable without a
 * DOM: {@link focusComposer} finds the composer under a surface and focuses it,
 * unless focus already sits somewhere inside that surface (so we never yank
 * focus off a dropdown/menu the user just opened). The IntersectionObserver +
 * requestAnimationFrame wiring that drives it lives in `AgentSessionSurface.tsx`.
 */

/** The class ProseMirror puts on its contenteditable host node. */
export const COMPOSER_SELECTOR = ".ProseMirror";

/** Minimal focusable shape — the ProseMirror contenteditable node. */
export interface ComposerFocusTarget {
  focus(): void;
}

/** Minimal surface shape: the agent-surface container element. */
export interface ComposerHost {
  querySelector(selector: string): ComposerFocusTarget | null;
  contains(node: unknown): boolean;
}

/** Minimal document shape: just the currently focused element. */
export interface ComposerFocusDoc {
  readonly activeElement: unknown;
}

/**
 * What {@link focusComposer} did.
 *
 * - `"focused"` — the composer was found and focused.
 * - `"already-in-surface"` — focus already lives inside the surface; left as-is.
 * - `"no-composer"` — the ProseMirror node is not in the DOM yet. The composer
 *   mounts asynchronously (the agent app fetches context before rendering), so
 *   callers should treat this as "retry next frame", not "give up".
 */
export type ComposerFocusOutcome = "focused" | "already-in-surface" | "no-composer";

/**
 * Focus the agent composer inside `surface`.
 *
 * Skips when focus is already somewhere within `surface` — the user may have
 * opened the provider / permissions / autocomplete menu, and stealing focus back
 * to the editor would close it. Returns {@link ComposerFocusOutcome} so the
 * caller can retry on `"no-composer"` while the editor is still mounting.
 */
export function focusComposer(surface: ComposerHost, doc: ComposerFocusDoc): ComposerFocusOutcome {
  const active = doc.activeElement;
  if (active != null && surface.contains(active)) {
    return "already-in-surface";
  }
  const composer = surface.querySelector(COMPOSER_SELECTOR);
  if (!composer) {
    return "no-composer";
  }
  composer.focus();
  return "focused";
}
