import { describe, expect, test } from "bun:test";

import type { ColorScheme } from "../settings/appearanceMode";
import {
  type AppearanceEnv,
  applyStoredAppearance,
  createDefaultAppearanceEnv,
} from "./useAppearance";

// `applyStoredAppearance` is the logic carrier — the React hook is a thin
// untested wrapper (bun test here has no hook renderer; component tests in
// this repo use renderToStaticMarkup, which never runs effects). Every case
// exercises the resolve → stamp → write-guard pipeline against the canonical
// `AppearanceSettings` semantics (apply on launch / on scheme change, the
// `:88` write-guard).

interface FakeEnv {
  env: AppearanceEnv;
  /** Every `applyDocumentColorScheme` call, in order. */
  applies: ColorScheme[];
  /** Every `writeStored` call, in order. */
  writes: string[];
  setScheme: (scheme: ColorScheme) => void;
  setStored: (raw: string | null) => void;
  /** Flips the ambient scheme and notifies subscribers. */
  flipScheme: () => void;
  subscriberCount: () => number;
}

function fakeEnv(opts?: {
  stored?: string | null;
  scheme?: ColorScheme;
}): FakeEnv {
  let stored: string | null = opts?.stored ?? null;
  let scheme: ColorScheme = opts?.scheme ?? "dark";
  const applies: ColorScheme[] = [];
  const writes: string[] = [];
  const subscribers = new Set<() => void>();
  const env: AppearanceEnv = {
    readStored: () => stored,
    writeStored: (raw) => {
      writes.push(raw);
      stored = raw;
    },
    systemColorScheme: () => scheme,
    subscribeSystemScheme: (onChange) => {
      subscribers.add(onChange);
      return () => subscribers.delete(onChange);
    },
    applyDocumentColorScheme: (s) => {
      applies.push(s);
    },
  };
  return {
    env,
    applies,
    writes,
    setScheme: (s) => {
      scheme = s;
    },
    setStored: (raw) => {
      stored = raw;
    },
    flipScheme: () => {
      scheme = scheme === "dark" ? "light" : "dark";
      for (const onChange of subscribers) onChange();
    },
    subscriberCount: () => subscribers.size,
  };
}

describe("applyStoredAppearance", () => {
  test("null stored + system dark → dark applied, rewrites to 'system'", () => {
    const f = fakeEnv({ stored: null, scheme: "dark" });
    const applied = applyStoredAppearance(f.env);
    expect(applied.colorScheme).toBe("dark");
    expect(applied.documentColorScheme).toBe("dark");
    expect(applied.followsSystem).toBe(true);
    expect(f.applies).toEqual(["dark"]);
    // Absent value differs from the resolved raw value → needsRewrite (the
    // :88 write-guard fires exactly once, with the normalized value).
    expect(f.writes).toEqual(["system"]);
  });

  test("stored 'system' → no writeStored call (write-guard negative)", () => {
    const f = fakeEnv({ stored: "system", scheme: "light" });
    const applied = applyStoredAppearance(f.env);
    expect(applied.needsRewrite).toBe(false);
    expect(f.writes).toEqual([]);
    expect(f.applies).toEqual(["light"]);
  });

  test("stored 'light' + system dark → doc 'light', no rewrite", () => {
    const f = fakeEnv({ stored: "light", scheme: "dark" });
    const applied = applyStoredAppearance(f.env);
    expect(applied.documentColorScheme).toBe("light");
    expect(applied.followsSystem).toBe(false);
    expect(f.applies).toEqual(["light"]);
    expect(f.writes).toEqual([]);
  });

  test("stored 'auto' → rewrites to 'system', follows system", () => {
    const f = fakeEnv({ stored: "auto", scheme: "dark" });
    const applied = applyStoredAppearance(f.env);
    expect(applied.mode).toBe("system");
    expect(applied.followsSystem).toBe(true);
    expect(f.applies).toEqual(["dark"]);
    expect(f.writes).toEqual(["system"]);
  });

  test("stored garbage → rewrites 'system', follows system", () => {
    const f = fakeEnv({ stored: "solarized", scheme: "light" });
    const applied = applyStoredAppearance(f.env);
    expect(applied.mode).toBe("system");
    expect(applied.followsSystem).toBe(true);
    expect(f.applies).toEqual(["light"]);
    expect(f.writes).toEqual(["system"]);
  });

  test("system flip while following system re-stamps the new scheme", () => {
    const f = fakeEnv({ stored: "system", scheme: "light" });
    applyStoredAppearance(f.env);
    expect(f.applies).toEqual(["light"]);
    f.setScheme("dark");
    applyStoredAppearance(f.env);
    expect(f.applies).toEqual(["light", "dark"]);
    expect(f.writes).toEqual([]);
  });

  test("system flip while stored 'dark' keeps stamping 'dark'", () => {
    const f = fakeEnv({ stored: "dark", scheme: "light" });
    applyStoredAppearance(f.env);
    f.setScheme("dark");
    applyStoredAppearance(f.env);
    expect(f.applies).toEqual(["dark", "dark"]);
    expect(f.writes).toEqual([]);
  });

  test("setStored flow: write then apply persists 'light' with no rewrite", () => {
    // Simulates `setStoredAppearance("light")` at the env level: the hook
    // writes the raw value first, then the apply pass normalizes.
    const f = fakeEnv({ stored: "system", scheme: "dark" });
    f.env.writeStored("light");
    const applied = applyStoredAppearance(f.env);
    expect(applied.documentColorScheme).toBe("light");
    expect(applied.persistedRawValue).toBe("light");
    // Only the explicit write; already canonical, so no rewrite followed.
    expect(f.writes).toEqual(["light"]);
  });

  test("fake subscribe contract: unsubscribe stops notifications", () => {
    const f = fakeEnv({ stored: "system", scheme: "light" });
    let notified = 0;
    const unsub = f.env.subscribeSystemScheme(() => {
      notified += 1;
    });
    f.flipScheme();
    expect(notified).toBe(1);
    unsub();
    expect(f.subscriberCount()).toBe(0);
    f.flipScheme();
    expect(notified).toBe(1);
  });
});

// `createDefaultAppearanceEnv` over hand-rolled fake `win`/`doc` objects —
// covers the media-query mapping, listener add/remove, the document stamp
// shape, and the try/catch storage guard, all without DOM globals.

interface FakeMediaQueryList {
  matches: boolean;
  listeners: Array<() => void>;
  addEventListener: (type: string, cb: () => void) => void;
  removeEventListener: (type: string, cb: () => void) => void;
}

function fakeWindow(opts?: {
  matches?: boolean;
  storage?: Pick<Storage, "getItem" | "setItem">;
}): { win: Window; mq: FakeMediaQueryList; store: Map<string, string> } {
  const store = new Map<string, string>();
  const mq: FakeMediaQueryList = {
    matches: opts?.matches ?? true,
    listeners: [],
    addEventListener: (_type, cb) => {
      mq.listeners.push(cb);
    },
    removeEventListener: (_type, cb) => {
      mq.listeners = mq.listeners.filter((l) => l !== cb);
    },
  };
  const localStorage = opts?.storage ?? {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
  };
  const win = {
    matchMedia: () => mq,
    localStorage,
  } as unknown as Window;
  return { win, mq, store };
}

function fakeDocument(): {
  doc: Document;
  style: Record<string, string>;
  dataset: Record<string, string>;
} {
  const style: Record<string, string> = {};
  const dataset: Record<string, string> = {};
  const doc = { documentElement: { style, dataset } } as unknown as Document;
  return { doc, style, dataset };
}

describe("createDefaultAppearanceEnv", () => {
  test("maps matchMedia matches → dark / light", () => {
    const dark = fakeWindow({ matches: true });
    const light = fakeWindow({ matches: false });
    const { doc } = fakeDocument();
    expect(
      createDefaultAppearanceEnv(dark.win, doc).systemColorScheme(),
    ).toBe("dark");
    expect(
      createDefaultAppearanceEnv(light.win, doc).systemColorScheme(),
    ).toBe("light");
  });

  test("reads/writes localStorage under 'appearanceMode'", () => {
    const { win, store } = fakeWindow();
    const env = createDefaultAppearanceEnv(win, fakeDocument().doc);
    expect(env.readStored()).toBeNull();
    env.writeStored("light");
    expect(store.get("appearanceMode")).toBe("light");
    expect(env.readStored()).toBe("light");
  });

  test("throwing localStorage degrades: read → null, write → no-op", () => {
    const { win } = fakeWindow({
      storage: {
        getItem: () => {
          throw new Error("denied");
        },
        setItem: () => {
          throw new Error("denied");
        },
      },
    });
    const env = createDefaultAppearanceEnv(win, fakeDocument().doc);
    expect(env.readStored()).toBeNull();
    expect(() => env.writeStored("dark")).not.toThrow();
  });

  test("stamps inline color-scheme style AND data-color-scheme dataset", () => {
    const { win } = fakeWindow();
    const { doc, style, dataset } = fakeDocument();
    const env = createDefaultAppearanceEnv(win, doc);
    env.applyDocumentColorScheme("light");
    expect(style["colorScheme"]).toBe("light");
    expect(dataset["colorScheme"]).toBe("light");
    env.applyDocumentColorScheme("dark");
    expect(style["colorScheme"]).toBe("dark");
    expect(dataset["colorScheme"]).toBe("dark");
  });

  test("subscribe adds a change listener; unsubscribe removes it", () => {
    const { win, mq } = fakeWindow();
    const env = createDefaultAppearanceEnv(win, fakeDocument().doc);
    const unsub = env.subscribeSystemScheme(() => {});
    expect(mq.listeners).toHaveLength(1);
    unsub();
    expect(mq.listeners).toHaveLength(0);
  });
});
