import { describe, expect, test } from "bun:test";

import {
  APPEARANCE_MODE_DEFAULT,
  APPEARANCE_MODE_DEFAULTS_KEY,
  appearanceColorScheme,
  appearanceColorSchemeOverride,
  appearanceModeFromRaw,
  normalizeAppearanceMode,
  resolvedAppearanceMode,
} from "./appearanceMode";

describe("appearanceMode constants", () => {
  test("default mode and defaults key mirror the Rust constants", () => {
    expect(APPEARANCE_MODE_DEFAULT).toBe("system");
    expect(APPEARANCE_MODE_DEFAULTS_KEY).toBe("appearanceMode");
  });
});

describe("appearanceModeFromRaw (mirrors AppearanceMode::from_raw)", () => {
  test("exact lowercase spellings parse", () => {
    expect(appearanceModeFromRaw("system")).toBe("system");
    expect(appearanceModeFromRaw("light")).toBe("light");
    expect(appearanceModeFromRaw("dark")).toBe("dark");
    expect(appearanceModeFromRaw("auto")).toBe("auto");
  });

  test("anything else does not parse", () => {
    expect(appearanceModeFromRaw("System")).toBeNull();
    expect(appearanceModeFromRaw("invalid")).toBeNull();
    expect(appearanceModeFromRaw("")).toBeNull();
  });
});

// Mirrors mode.rs `normalize_collapses_auto_and_unknown_to_system`.
describe("normalizeAppearanceMode", () => {
  test("auto / unknown / null / undefined collapse to system", () => {
    expect(normalizeAppearanceMode("system")).toBe("system");
    expect(normalizeAppearanceMode("light")).toBe("light");
    expect(normalizeAppearanceMode("dark")).toBe("dark");
    expect(normalizeAppearanceMode("auto")).toBe("system");
    expect(normalizeAppearanceMode("totally-unknown")).toBe("system");
    expect(normalizeAppearanceMode(null)).toBe("system");
    expect(normalizeAppearanceMode(undefined)).toBe("system");
  });

  // Mirrors mode.rs `normalize_is_idempotent`.
  test("is idempotent: normalizing its own result is a fixed point", () => {
    for (const raw of ["system", "light", "dark", "auto", "unknown"]) {
      const once = normalizeAppearanceMode(raw);
      const twice = normalizeAppearanceMode(once);
      expect(twice).toBe(once);
    }
    for (const mode of ["system", "light", "dark"] as const) {
      expect(normalizeAppearanceMode(mode)).toBe(mode);
    }
  });
});

// Mirrors mode.rs
// `color_scheme_override_is_explicit_only_for_manual_light_and_dark_modes`.
describe("appearanceColorSchemeOverride", () => {
  test("override is explicit only for manual light and dark modes", () => {
    expect(appearanceColorSchemeOverride("light")).toBe("light");
    expect(appearanceColorSchemeOverride("dark")).toBe("dark");
    expect(appearanceColorSchemeOverride("system")).toBeNull();
    expect(appearanceColorSchemeOverride("auto")).toBeNull();
    expect(appearanceColorSchemeOverride("invalid")).toBeNull();
  });

  // Mirrors mode.rs `color_scheme_override_defers_on_none`.
  test("null / undefined defer to ambient (no override)", () => {
    expect(appearanceColorSchemeOverride(null)).toBeNull();
    expect(appearanceColorSchemeOverride(undefined)).toBeNull();
  });
});

describe("appearanceColorScheme (override else fallback)", () => {
  test("manual mode wins over fallback; ambient modes use fallback", () => {
    expect(appearanceColorScheme("dark", "light")).toBe("dark");
    expect(appearanceColorScheme("system", "dark")).toBe("dark");
    expect(appearanceColorScheme(null, "light")).toBe("light");
    expect(appearanceColorScheme(null, "dark")).toBe("dark");
    expect(appearanceColorScheme(undefined, "light")).toBe("light");
  });
});

// Mirrors mode.rs `resolved_mode_flags_rewrite_when_stored_differs`.
describe("resolvedAppearanceMode", () => {
  test("flags a rewrite exactly when stored differs from the resolved value", () => {
    expect(resolvedAppearanceMode(undefined)).toEqual(["system", true]);
    expect(resolvedAppearanceMode(null)).toEqual(["system", true]);
    expect(resolvedAppearanceMode("auto")).toEqual(["system", true]);
    expect(resolvedAppearanceMode("bogus")).toEqual(["system", true]);
    expect(resolvedAppearanceMode("system")).toEqual(["system", false]);
    expect(resolvedAppearanceMode("light")).toEqual(["light", false]);
    expect(resolvedAppearanceMode("dark")).toEqual(["dark", false]);
  });
});
