// Appearance-mode normalization — faithful port of
// `crates/cmux-appearance/src/mode.rs` (itself a port of
// `Sources/AppearanceSettings.swift`).
//
// `AppearanceMode` is a DISTINCT 4-case enum (`system` / `light` / `dark` /
// `auto`) and must NOT be confused with the generated `@cmux/core-types`
// `Appearance` (the 3-case config-file enum: `system` / `light` / `dark`).
// This layer *resolves* between the persisted `appearanceMode` user-defaults
// string and an explicit light/dark color-scheme override.
//
// All logic is pure; the persisted `appearanceMode` value is injected as a
// `string | null | undefined` (the caller owns the read/write), mirroring the
// Rust `Option<&str>`.
//
// DIVERGENCE: Rust models the modes as an `enum` with `raw_value()`. In
// TypeScript we represent the mode directly as its lowercase raw-value string
// literal union (the serde `rename_all = "lowercase"` spelling), so the value
// and its raw value are identical — no separate `raw_value()` mapping needed.

/**
 * The app-wide appearance mode persisted under the `appearanceMode`
 * user-defaults key. Mirrors Rust `AppearanceMode` / Swift `AppearanceMode`.
 *
 * Includes `"auto"` for wire/back-compat: `"auto"` is accepted on read but
 * always collapses to `"system"` during normalization, so it never survives
 * into resolved state.
 */
export type AppearanceMode = "system" | "light" | "dark" | "auto";

/**
 * The concrete light/dark color scheme, mirroring Rust `ColorScheme` /
 * SwiftUI's `ColorScheme`. DISTINCT from the terminal-domain preference.
 */
export type ColorScheme = "light" | "dark";

/** The default mode when none is stored. Mirrors `AppearanceMode::DEFAULT`. */
export const APPEARANCE_MODE_DEFAULT: AppearanceMode = "system";

/**
 * The persisted user-defaults key. Mirrors `AppearanceMode::DEFAULTS_KEY` /
 * `AppearanceSettings.appearanceModeKey`.
 */
export const APPEARANCE_MODE_DEFAULTS_KEY = "appearanceMode";

/**
 * Exact-match parse of a persisted raw value, mirroring
 * `AppearanceMode::from_raw` / `AppearanceMode(rawValue:)`. Only the exact
 * lowercase spellings match; everything else yields `null`.
 */
export function appearanceModeFromRaw(raw: string): AppearanceMode | null {
  switch (raw) {
    case "system":
      return "system";
    case "light":
      return "light";
    case "dark":
      return "dark";
    case "auto":
      return "auto";
    default:
      return null;
  }
}

/**
 * Normalizes a persisted `appearanceMode` raw value into a resolved mode,
 * mirroring `normalize` / `AppearanceSettings.mode(for:)`.
 *
 * Unknown values, `null`, and `undefined` fall back to the `"system"` default;
 * `"auto"` collapses to `"system"`. The result is therefore always one of
 * `"system"` / `"light"` / `"dark"`.
 */
export function normalizeAppearanceMode(
  raw: string | null | undefined,
): AppearanceMode {
  const parsed = raw == null ? null : appearanceModeFromRaw(raw);
  if (parsed === "auto") {
    return "system";
  }
  if (parsed === null) {
    return APPEARANCE_MODE_DEFAULT;
  }
  return parsed;
}

/**
 * Returns the explicit color-scheme override for a persisted raw value, or
 * `null` when the mode follows the system/ambient scheme. Mirrors
 * `color_scheme_override` / `AppearanceSettings.colorSchemeOverride(for:)`.
 *
 * Only manual `"light"` / `"dark"` yield an override; `"system"`, `"auto"`,
 * unknown, `null`, and `undefined` all yield `null` (they defer to ambient).
 */
export function appearanceColorSchemeOverride(
  raw: string | null | undefined,
): ColorScheme | null {
  switch (normalizeAppearanceMode(raw)) {
    case "light":
      return "light";
    case "dark":
      return "dark";
    default:
      // `"system"` (and, via normalize, `"auto"`/unknown/null) defer to ambient.
      return null;
  }
}

/**
 * Resolves the effective color scheme for a persisted raw value, falling back
 * to `fallback` when the mode has no explicit override. Mirrors
 * `color_scheme` / `AppearanceSettings.colorScheme(for:fallback:)`.
 */
export function appearanceColorScheme(
  raw: string | null | undefined,
  fallback: ColorScheme,
): ColorScheme {
  return appearanceColorSchemeOverride(raw) ?? fallback;
}

/**
 * Pure counterpart of `resolved_mode` / `AppearanceSettings.resolvedMode`.
 *
 * Given the currently-stored raw value, returns the normalized mode plus
 * whether the store needs rewriting so it matches the resolved raw value (i.e.
 * the stored string was absent, unknown, or `"auto"`). The caller owns the
 * write — this performs no I/O.
 */
export function resolvedAppearanceMode(
  stored: string | null | undefined,
): [AppearanceMode, boolean] {
  const resolved = normalizeAppearanceMode(stored);
  // Rust: `stored != Some(resolved.raw_value())`. Here the mode IS its raw
  // value, so compare directly; `null`/`undefined` always differ from the
  // non-null resolved string.
  const needsRewrite = stored !== resolved;
  return [resolved, needsRewrite];
}
