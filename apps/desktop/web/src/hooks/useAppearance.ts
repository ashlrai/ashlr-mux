// The thin E9 caller over the pure `resolveAppliedAppearance` producer: watch
// the stored appearance mode + the ambient system scheme, stamp the resolved
// concrete scheme on `:root`, and surface the one-shot normalization
// write-back (`needsRewrite`, e.g. legacy "auto" → "system") to the owner of
// the config store. All decisions stay in the pure module; this hook only
// reads `matchMedia` and mutates the document.

import { useEffect, useState } from "react";

import type { ColorScheme } from "../settings/appearanceMode";
import {
  resolveAppliedAppearance,
  type AppliedAppearance,
} from "../settings/appearanceResolve";

/** The ambient system scheme, `"dark"` when `matchMedia` is unavailable
 * (tests / SSR — the shell's baseline theme). */
function readSystemScheme(): ColorScheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "dark";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

/**
 * Applies `stored` (the persisted `app.appearance` raw value) to the document
 * root and keeps it applied across system-scheme changes. When the resolver
 * reports `needsRewrite`, `onRewrite` is invoked ONCE per stored value with
 * the normalized mode so the caller can persist it (the Swift
 * `stored != resolved.rawValue` write-guard, AppearanceSettings.swift:88).
 */
export function useAppearance(
  stored: string | null | undefined,
  onRewrite?: (persistedRawValue: AppliedAppearance["persistedRawValue"]) => void,
): void {
  const [systemScheme, setSystemScheme] = useState<ColorScheme>(readSystemScheme);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (event: MediaQueryListEvent): void => {
      setSystemScheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    const applied = resolveAppliedAppearance(stored, systemScheme);
    const root = document.documentElement;
    // The concrete scheme is the only theme token the shell consumes today
    // (styles.css `:root { color-scheme }`); the data attribute is the hook
    // for future scheme-conditional CSS.
    root.style.colorScheme = applied.documentColorScheme;
    root.dataset.colorScheme = applied.documentColorScheme;
    if (applied.needsRewrite && onRewrite) {
      onRewrite(applied.persistedRawValue);
    }
    // `onRewrite` deliberately omitted: rewriting is keyed to the stored
    // value, not to callback identity (a per-render closure would refire).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stored, systemScheme]);
}
