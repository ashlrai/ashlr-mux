import { describe, expect, test } from "bun:test";

import { COMPOSER_SELECTOR, type ComposerHost, focusComposer } from "./composerFocus";

/** A fake surface whose composer node records whether it was focused. */
function makeSurface(options: {
  composer?: { focus: () => void } | null;
  contains?: (node: unknown) => boolean;
}): ComposerHost {
  return {
    querySelector: (selector: string) =>
      selector === COMPOSER_SELECTOR ? (options.composer ?? null) : null,
    contains: options.contains ?? (() => false),
  };
}

describe("focusComposer", () => {
  test("focuses the composer when nothing in the surface is focused", () => {
    let focused = false;
    const surface = makeSurface({ composer: { focus: () => (focused = true) } });

    expect(focusComposer(surface, { activeElement: null })).toBe("focused");
    expect(focused).toBe(true);
  });

  test("focuses even when some unrelated element is active (activeElement outside surface)", () => {
    let focused = false;
    const surface = makeSurface({
      composer: { focus: () => (focused = true) },
      contains: () => false,
    });
    const somethingElse = { tag: "terminal" };

    expect(focusComposer(surface, { activeElement: somethingElse })).toBe("focused");
    expect(focused).toBe(true);
  });

  test("leaves focus alone when it already lives inside the surface (open menu / editor)", () => {
    let focused = false;
    const openMenuItem = { tag: "provider-menu" };
    const surface = makeSurface({
      composer: { focus: () => (focused = true) },
      contains: (node) => node === openMenuItem,
    });

    expect(focusComposer(surface, { activeElement: openMenuItem })).toBe("already-in-surface");
    expect(focused).toBe(false);
  });

  test("reports no-composer (retry signal) while the editor is still mounting", () => {
    const surface = makeSurface({ composer: null });

    expect(focusComposer(surface, { activeElement: null })).toBe("no-composer");
  });
});
