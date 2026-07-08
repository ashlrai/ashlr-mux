// E3 — the delta representation between a Settings mutation and the on-disk
// `cmux.json`: every `ConfigAction` maps to ONE dotted-JSONPath write, applied
// by the Rust `config_set` / `config_remove` commands onto the RAW tree. The
// UI state moves through `configReducer`; the FILE moves through this mapper —
// both derive from the same action, so they cannot disagree about what
// changed, and the file stays overrides-only (defaults never serialize).

import type { Config } from "@cmux/core-types";

import type { ConfigAction } from "./configReducer";

/** One file mutation: set a dotted path, or remove it. */
export type ConfigDelta =
  | { kind: "set"; path: string; value: unknown }
  | { kind: "remove"; path: string };

/**
 * The file delta for applying `action` against `config` (the SAME config the
 * reducer sees — pass the effective, default-filled view so toggle values
 * flip off the effective state). Returns `null` exactly when `configReducer`
 * would no-op (absent target section), keeping state and file in lockstep.
 */
export function deltaForAction(
  config: Config,
  action: ConfigAction,
): ConfigDelta | null {
  switch (action.type) {
    case "toggleSidebarFlag": {
      const sidebar = config.sidebar;
      if (!sidebar) {
        return null;
      }
      return {
        kind: "set",
        path: `sidebar.${action.key}`,
        value: !sidebar[action.key],
      };
    }
    case "toggleNotificationsFlag": {
      const notifications = config.notifications;
      if (!notifications) {
        return null;
      }
      return {
        kind: "set",
        path: `notifications.${action.key}`,
        value: !notifications[action.key],
      };
    }
    case "setAppearance": {
      if (!config.app) {
        return null;
      }
      return { kind: "set", path: "app.appearance", value: action.appearance };
    }
    case "setShortcutBinding": {
      if (!config.shortcuts) {
        return null;
      }
      // NOTE: shortcut action ids are single tokens (no "."); an id with a
      // dot would split into path segments — guard rather than corrupt.
      if (action.action.includes(".")) {
        return null;
      }
      // `null` is the EXPLICIT unbind (distinct from absent) — mirrored from
      // the reducer's `Option<ShortcutBinding>` contract.
      return {
        kind: "set",
        path: `shortcuts.bindings.${action.action}`,
        value: action.binding,
      };
    }
  }
}
