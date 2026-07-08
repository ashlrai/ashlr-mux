// React binding for the cmux.json commands (`src-tauri/src/config.rs`).
//
// Load: `config_load` returns the RAW tree; the hook presents the EFFECTIVE
// view (`withSettingsDefaults`) so the Settings UI always sees present
// sections. Mutate: one `ConfigAction` drives BOTH the optimistic in-memory
// `configReducer` step AND the single dotted-path file write
// (`deltaForAction` → `config_set`/`config_remove`); the authoritative tree
// returned by the command (and every `cmux://config-changed` broadcast, e.g.
// from another window) reconciles the state. Defaults never reach the file.

import { useCallback, useEffect, useRef, useState } from "react";

import type { Config } from "@cmux/core-types";

import { host } from "../host/host";
import { withSettingsDefaults } from "../settings/configDefaults";
import { deltaForAction } from "../settings/configDelta";
import { configReducer, type ConfigAction } from "../settings/configReducer";

export interface UseConfig {
  /** Effective (default-filled) config, or `null` until the first load. */
  config: Config | null;
  /** Apply a Settings mutation: optimistic state + persisted file delta. */
  dispatch: (action: ConfigAction) => void;
}

export function useConfig(): UseConfig {
  const [config, setConfig] = useState<Config | null>(null);
  // Ref mirror so `dispatch` reads the latest state without re-binding.
  const configRef = useRef<Config | null>(null);
  configRef.current = config;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void host
      .invoke<Config>("config_load")
      .then((raw) => {
        if (!disposed) {
          setConfig(withSettingsDefaults(raw));
        }
      })
      .catch(() => {
        // No host (plain-browser dev) or unreadable file: present defaults so
        // the pane still renders; writes will surface their own errors.
        if (!disposed) {
          setConfig(withSettingsDefaults({}));
        }
      });

    void host
      .on<Config>("cmux://config-changed", (raw) => {
        if (!disposed) {
          setConfig(withSettingsDefaults(raw));
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const dispatch = useCallback((action: ConfigAction) => {
    const prev = configRef.current;
    if (!prev) {
      return;
    }
    // Optimistic: the reducer's next state, immediately.
    setConfig(configReducer(prev, action));
    // Durable: the same action as one dotted-path file write.
    const delta = deltaForAction(prev, action);
    if (!delta) {
      return;
    }
    const invocation =
      delta.kind === "set"
        ? host.invoke<Config>("config_set", { path: delta.path, value: delta.value })
        : host.invoke<Config>("config_remove", { path: delta.path });
    void invocation
      .then((raw) => setConfig(withSettingsDefaults(raw)))
      .catch((error) => console.error("config write failed", delta.path, error));
  }, []);

  return { config, dispatch };
}
