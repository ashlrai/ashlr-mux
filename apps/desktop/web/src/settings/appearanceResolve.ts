// Appearance application resolver — faithful pure port of
// `AppearanceSettings.applicationAppearance(for:duringLaunch:environment:)`
// (`Sources/AppearanceSettings.swift:198-213`) and the apply-site semantics of
// `AppearanceColorSchemeModifier` (`:312-323`).
//
// This is the ONE pure decision missing from `appearanceMode.ts`: it folds a
// normalized mode + concrete system color scheme into the exact triple a thin
// host caller stamps on the document root, plus the persist delta.
//
// It COMPOSES the already-ported pure fns from `./appearanceMode` — it does NOT
// re-implement the light/dark/system branch (that would be the duplication
// trap; `appearanceColorScheme` already performs the auto+system fold).
//
// DEFERRED / OUT OF SCOPE (the thin host side-effect tail):
//   - the actual `document.documentElement` mutation (stamping
//     `color-scheme` / a future data-theme attribute), and
//   - the `localStorage` / user-defaults write-back to
//     `APPEARANCE_MODE_DEFAULTS_KEY` guarded by `needsRewrite`.
// This module performs NO DOM access, NO Tauri IPC, and NO global reads — the
// concrete system scheme is injected as `systemColorScheme` (the caller's
// `matchMedia('(prefers-color-scheme: dark)')` read, the Windows analogue of
// macOS `SystemAppearance.current` / `AppleInterfaceStyle`).
//
// PARITY NUANCE — `duringLaunch` is host-only. Canonical
// `applicationAppearance` returns `systemAppearance()` for `.system` only
// `duringLaunch`, else `nil`; both collapse to "follow ambient" for a pure
// producer. We represent "follow ambient" via `followsSystem` (mirroring the
// `applicationAppearance` `nil` return) plus a concrete `colorScheme` derived
// from the supplied `systemColorScheme`. We do NOT port `duringLaunch`.

import {
  type AppearanceMode,
  type ColorScheme,
  appearanceColorScheme,
  appearanceColorSchemeOverride,
  resolvedAppearanceMode,
} from "./appearanceMode";

/**
 * The applied appearance result — everything a thin host caller needs to stamp
 * the document root and persist the resolved mode, packaged as one pure value.
 *
 * Mirrors the effective decision of `applicationAppearance(for:)` +
 * `AppearanceColorSchemeModifier`: `followsSystem` is the `nil`-return
 * discriminator (defer to ambient), and `colorScheme` /
 * `documentColorScheme` are the concrete scheme to force/follow.
 */
export interface AppliedAppearance {
  /**
   * The normalized appearance mode. Never `"auto"` — normalization always
   * collapses `"auto"` to `"system"` (`mode.rs:82`, `appearanceMode.ts:77-79`).
   */
  mode: AppearanceMode;
  /**
   * The effective color scheme = `appearanceColorScheme(stored,
   * systemColorScheme)`: the explicit light/dark override when the mode is
   * manual, otherwise the supplied system scheme. Already folds
   * "auto/system + system-dark → dark".
   */
  colorScheme: ColorScheme;
  /**
   * Whether the effective scheme follows the ambient system scheme rather than
   * a forced override. `true` ⇔ `appearanceColorSchemeOverride(stored) ===
   * null` (system/auto/unknown/null), mirroring `applicationAppearance`
   * returning `nil`. This is the "force `.aqua`/`.darkAqua` vs. defer"
   * discriminator.
   */
  followsSystem: boolean;
  /**
   * The concrete scheme the host stamps on `:root`. Equal to `colorScheme`:
   * the web must resolve to a concrete light/dark because CSS `color-scheme` at
   * `:root` is currently the only theme token (`styles.css:3-4`), with no
   * data-theme / token vocabulary — so we emit the concrete scheme rather than
   * inventing an attribute the canonical layer lacks.
   */
  documentColorScheme: ColorScheme;
  /**
   * The raw value to persist under `APPEARANCE_MODE_DEFAULTS_KEY`. Equal to
   * `mode` (the mode IS its raw value in this layer).
   */
  persistedRawValue: AppearanceMode;
  /**
   * Whether the store needs rewriting so it matches the resolved raw value —
   * from `resolvedAppearanceMode`, mirroring Swift's `stored !=
   * resolved.rawValue` write-guard (`AppearanceSettings.swift:88`). The thin
   * caller writes back ONLY when this is `true`.
   */
  needsRewrite: boolean;
}

/**
 * Folds a persisted appearance-mode raw value + the concrete ambient system
 * color scheme into the applied result: the normalized mode, the effective and
 * document color schemes, whether it follows the system, and the persist delta.
 *
 * Pure and cross-platform; performs no DOM / IPC / global access. Mirrors
 * `applicationAppearance(for:)` collapsed to a pure producer (see file header
 * for the `duringLaunch` note).
 *
 * @param stored The persisted `appearanceMode` raw value (`string | null |
 *   undefined`), owned/read by the caller — mirrors the Rust `Option<&str>`.
 * @param systemColorScheme The concrete ambient system scheme (the caller's
 *   `matchMedia` read); used as the fallback when the mode defers to ambient.
 */
export function resolveAppliedAppearance(
  stored: string | null | undefined,
  systemColorScheme: ColorScheme,
): AppliedAppearance {
  const [mode, needsRewrite] = resolvedAppearanceMode(stored);
  const colorScheme = appearanceColorScheme(stored, systemColorScheme);
  const followsSystem = appearanceColorSchemeOverride(stored) === null;
  return {
    mode,
    colorScheme,
    followsSystem,
    documentColorScheme: colorScheme,
    persistedRawValue: mode,
    needsRewrite,
  };
}
