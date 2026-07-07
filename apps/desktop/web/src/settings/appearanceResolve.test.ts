import { describe, expect, test } from "bun:test";

import { resolveAppliedAppearance } from "./appearanceResolve";

// Oracle cases mirror `AppearanceSettings.applicationAppearance(for:)`
// (`Sources/AppearanceSettings.swift:198-213`) collapsed to a pure producer,
// composed over the already-ported `appearanceMode.ts` fns. Each case asserts
// the { mode, colorScheme, followsSystem, needsRewrite } quadruple.

describe("resolveAppliedAppearance", () => {
  // auto never survives normalization — collapses to "system"; auto→system is
  // a persisted delta (mirrors resolved_mode(Some("auto"))).
  test("auto + system-dark → system, dark, follows system, rewrite", () => {
    const r = resolveAppliedAppearance("auto", "dark");
    expect(r.mode).toBe("system");
    expect(r.colorScheme).toBe("dark");
    expect(r.followsSystem).toBe(true);
    expect(r.needsRewrite).toBe(true);
  });

  test("auto + system-light → system, light, follows system, rewrite", () => {
    const r = resolveAppliedAppearance("auto", "light");
    expect(r.mode).toBe("system");
    expect(r.colorScheme).toBe("light");
    expect(r.followsSystem).toBe(true);
    expect(r.needsRewrite).toBe(true);
  });

  // system follows the ambient scheme; already-canonical, so no rewrite.
  test("system + system-dark → system, dark, follows system, no rewrite", () => {
    const r = resolveAppliedAppearance("system", "dark");
    expect(r.mode).toBe("system");
    expect(r.colorScheme).toBe("dark");
    expect(r.followsSystem).toBe(true);
    expect(r.needsRewrite).toBe(false);
  });

  test("system + system-light → system, light, follows system, no rewrite", () => {
    const r = resolveAppliedAppearance("system", "light");
    expect(r.mode).toBe("system");
    expect(r.colorScheme).toBe("light");
    expect(r.followsSystem).toBe(true);
    expect(r.needsRewrite).toBe(false);
  });

  // explicit override wins over the ambient system scheme (mirrors
  // explicit_light_mode_wins); not following system; already canonical.
  test("explicit light + system-dark → light forced, no rewrite", () => {
    const r = resolveAppliedAppearance("light", "dark");
    expect(r.mode).toBe("light");
    expect(r.colorScheme).toBe("light");
    expect(r.followsSystem).toBe(false);
    expect(r.needsRewrite).toBe(false);
  });

  test("explicit dark + system-light → dark forced, no rewrite", () => {
    const r = resolveAppliedAppearance("dark", "light");
    expect(r.mode).toBe("dark");
    expect(r.colorScheme).toBe("dark");
    expect(r.followsSystem).toBe(false);
    expect(r.needsRewrite).toBe(false);
  });

  // null / undefined / unknown all resolve to system + rewrite (mirrors
  // resolved_mode(None/bogus)); the emitted colorScheme follows the supplied
  // system scheme.
  test("null / undefined / unknown + system-dark → system, dark, rewrite", () => {
    for (const stored of [null, undefined, "totally-unknown"] as const) {
      const r = resolveAppliedAppearance(stored, "dark");
      expect(r.mode).toBe("system");
      expect(r.colorScheme).toBe("dark");
      expect(r.followsSystem).toBe(true);
      expect(r.needsRewrite).toBe(true);
    }
  });

  // documentColorScheme is the concrete scheme to stamp; equals colorScheme.
  test("documentColorScheme equals the effective colorScheme", () => {
    expect(resolveAppliedAppearance("auto", "dark").documentColorScheme).toBe(
      "dark",
    );
    expect(resolveAppliedAppearance("light", "dark").documentColorScheme).toBe(
      "light",
    );
    expect(resolveAppliedAppearance("system", "light").documentColorScheme).toBe(
      "light",
    );
  });

  // persistedRawValue equals the normalized mode (never "auto").
  test("persistedRawValue equals the normalized mode", () => {
    expect(resolveAppliedAppearance("auto", "dark").persistedRawValue).toBe(
      "system",
    );
    expect(resolveAppliedAppearance("dark", "light").persistedRawValue).toBe(
      "dark",
    );
  });

  // named-theme override: the emitted colorScheme is the light|dark key you
  // would feed into resolve_theme_name(raw, pref) to select the concrete theme
  // token. We assert the mapping, NOT a re-implementation of theme-name
  // resolution (that lives in the separate theme_selection codec).
  test("colorScheme is the light|dark selector for a light:X,dark:Y choice", () => {
    const pick = (stored: string | null | undefined, sys: "light" | "dark") =>
      resolveAppliedAppearance(stored, sys).colorScheme === "dark"
        ? "dark-token"
        : "light-token";
    expect(pick("light", "dark")).toBe("light-token"); // override wins
    expect(pick("dark", "light")).toBe("dark-token"); // override wins
    expect(pick("system", "dark")).toBe("dark-token"); // follows system
    expect(pick("auto", "light")).toBe("light-token"); // auto→system→ambient
  });

  // idempotence / persist: needsRewrite is true exactly for the non-canonical
  // raw values (null | undefined | "auto" | unknown), false for the canonical
  // spellings — independent of the supplied system scheme.
  test("needsRewrite is true exactly for non-canonical stored values", () => {
    for (const sys of ["light", "dark"] as const) {
      for (const stored of [null, undefined, "auto", "bogus"] as const) {
        expect(resolveAppliedAppearance(stored, sys).needsRewrite).toBe(true);
      }
      for (const stored of ["system", "light", "dark"] as const) {
        expect(resolveAppliedAppearance(stored, sys).needsRewrite).toBe(false);
      }
    }
  });
});
