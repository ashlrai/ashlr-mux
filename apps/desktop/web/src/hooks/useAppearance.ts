// Appearance apply path — the thin host tail deferred by
// `../settings/appearanceResolve.ts` (its header: "DEFERRED... the actual
// `document.documentElement` mutation... and the `localStorage` write-back
// guarded by `needsRewrite`"). Canonical semantics:
// `AppearanceSettings.applicationAppearance(for:duringLaunch:)`
// (`Sources/AppearanceSettings.swift:198-213`) applied by
// `AppearanceColorSchemeModifier` (`:312-323`) on launch and on system-scheme
// change, with the `stored != resolved.rawValue` write-guard (`:88`)
// persisting the normalized raw value.
//
// Layering: `AppearanceEnv` is the injectable host surface (storage + media
// query + document stamp), `applyStoredAppearance` is the pure-given-env core
// that carries all the logic (and all the tests), and `useAppearance` is a
// thin React wrapper that runs it on mount and on ambient-scheme changes.

import { useCallback, useEffect, useState } from "react";

import {
  APPEARANCE_MODE_DEFAULTS_KEY,
  type ColorScheme,
} from "../settings/appearanceMode";
import {
  type AppliedAppearance,
  resolveAppliedAppearance,
} from "../settings/appearanceResolve";

/**
 * The injectable host surface for the appearance apply path. Everything the
 * apply logic touches — persisted value, ambient system scheme, document
 * root — goes through this interface so the logic is testable without a DOM.
 */
export interface AppearanceEnv {
  /** Raw persisted `appearanceMode` value, or `null` when absent/unreadable. */
  readStored(): string | null;
  /** Persists the raw `appearanceMode` value. */
  writeStored(raw: string): void;
  /** The concrete ambient system color scheme. */
  systemColorScheme(): ColorScheme;
  /**
   * Subscribes to ambient system-scheme changes; returns the unsubscribe.
   * The callback carries no payload — re-read via {@link systemColorScheme}.
   */
  subscribeSystemScheme(onChange: () => void): () => void;
  /** Stamps the concrete scheme on the document root. */
  applyDocumentColorScheme(scheme: ColorScheme): void;
}

/**
 * The real host wiring of {@link AppearanceEnv} over `window` / `document`.
 *
 * DIVERGENCE: canonical persists `appearanceMode` in NSUserDefaults
 * (`AppearanceSettings.appearanceModeKey`); here `localStorage` under the
 * same key string is the interim store until a settings-persistence surface
 * lands (none exists today — no localStorage use elsewhere in web src, no
 * settings store command in src-tauri). Revisit when settings persistence
 * lands.
 */
export function createDefaultAppearanceEnv(
  win: Window = window,
  doc: Document = document,
): AppearanceEnv {
  const mq = win.matchMedia("(prefers-color-scheme: dark)");
  return {
    // try/catch: a storage-denied webview (privacy mode / policy) throws on
    // localStorage access; that must degrade to defaults, not crash the shell.
    readStored: () => {
      try {
        return win.localStorage.getItem(APPEARANCE_MODE_DEFAULTS_KEY);
      } catch {
        return null;
      }
    },
    writeStored: (raw) => {
      try {
        win.localStorage.setItem(APPEARANCE_MODE_DEFAULTS_KEY, raw);
      } catch {
        // Persist is best-effort; the applied scheme still holds in-memory.
      }
    },
    systemColorScheme: () => (mq.matches ? "dark" : "light"),
    subscribeSystemScheme: (onChange) => {
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    },
    applyDocumentColorScheme: (scheme) => {
      // Inline style deliberately overrides the hardcoded
      // `:root { color-scheme: dark }` in styles.css:3-4.
      doc.documentElement.style.colorScheme = scheme;
      // Renders as `data-color-scheme="light|dark"` — an additive divergence:
      // the canonical layer has no data-theme vocabulary (see
      // appearanceResolve.ts on `documentColorScheme`); this is a token hook
      // for future light-theme CSS.
      doc.documentElement.dataset.colorScheme = scheme;
    },
  };
}

/**
 * The apply pass: resolve the stored mode against the ambient scheme, stamp
 * the document root, and persist the normalized raw value ONLY when
 * `needsRewrite` (the `AppearanceSettings.swift:88` write-guard). Pure given
 * the env; this is the tested logic carrier behind {@link useAppearance}.
 */
export function applyStoredAppearance(env: AppearanceEnv): AppliedAppearance {
  const applied = resolveAppliedAppearance(
    env.readStored(),
    env.systemColorScheme(),
  );
  env.applyDocumentColorScheme(applied.documentColorScheme);
  if (applied.needsRewrite) {
    env.writeStored(applied.persistedRawValue);
  }
  return applied;
}

export interface UseAppearance {
  /** The currently applied appearance. */
  applied: AppliedAppearance;
  /**
   * Persists a raw `appearanceMode` value and re-applies (the apply pass
   * normalizes + rewrites via `needsRewrite`, so e.g. `"auto"` net-persists
   * as `"system"`). Exported for a LATER SettingsPane lane. NOTE for that
   * lane: the existing SettingsPane Appearance radio group
   * (SettingsPane.tsx:194-205) is bound to the generated 3-case config-file
   * `Appearance` enum via configReducer — a DIFFERENT surface from this
   * `appearanceMode` user-default (appearanceMode.ts:5-9); do not conflate
   * them when wiring.
   */
  setStoredAppearance: (raw: string) => void;
}

/**
 * Applies the persisted appearance on mount and re-applies on ambient
 * system-scheme changes — the web analogue of `AppearanceColorSchemeModifier`
 * applying on launch and on system-appearance change. Re-applying on a scheme
 * change while the mode is a manual override is a no-op by value (the
 * resolver returns the same override), matching the canonical modifier just
 * re-evaluating.
 */
export function useAppearance(env?: AppearanceEnv): UseAppearance {
  // Resolve the env ONCE, lazily — a default-param call would rebuild it
  // (and re-run matchMedia) on every render.
  const [resolvedEnv] = useState(() => env ?? createDefaultAppearanceEnv());
  // Initializer reads only — no side effects during render (React purity);
  // the document stamp + rewrite happen in the mount effect. The one-frame
  // delay is invisible: the default CSS is already dark.
  const [applied, setApplied] = useState<AppliedAppearance>(() =>
    resolveAppliedAppearance(
      resolvedEnv.readStored(),
      resolvedEnv.systemColorScheme(),
    ),
  );

  useEffect(() => {
    setApplied(applyStoredAppearance(resolvedEnv));
    return resolvedEnv.subscribeSystemScheme(() => {
      setApplied(applyStoredAppearance(resolvedEnv));
    });
  }, [resolvedEnv]);

  const setStoredAppearance = useCallback(
    (raw: string) => {
      resolvedEnv.writeStored(raw);
      setApplied(applyStoredAppearance(resolvedEnv));
    },
    [resolvedEnv],
  );

  return { applied, setStoredAppearance };
}
