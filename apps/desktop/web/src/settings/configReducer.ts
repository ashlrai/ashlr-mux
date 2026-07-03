// Pure, immutable updater over the generated `@cmux/core-types` `Config`.
//
// Every action returns a NEW `Config` and NEVER mutates its input (structural
// sharing: untouched sections keep their reference identity). This is the
// desktop-web counterpart of the Settings mutations the macOS app performs on
// its in-memory `CmuxConfigFile` before persisting `cmux.json`.
//
// Sections on `Config` are all optional and their sub-shapes have required
// fields, so a section that is absent cannot be synthesized from a partial
// value. When an action targets an absent section the reducer is a no-op
// (returns the input unchanged) — the Settings UI only dispatches against a
// loaded config whose relevant section is present.

import type {
  Appearance,
  Config,
  NotificationsConfig,
  ShortcutBinding,
  SidebarConfig,
} from "@cmux/core-types";

/** The keys of `T` whose value type is exactly `boolean`. */
type BooleanKeys<T> = {
  [K in keyof T]-?: T[K] extends boolean ? K : never;
}[keyof T];

/** Toggleable boolean flags on `sidebar` (excludes `branchLayout`, widths). */
export type SidebarBoolKey = BooleanKeys<SidebarConfig>;

/** Toggleable boolean flags on `notifications` (excludes sound/command/etc). */
export type NotificationsBoolKey = BooleanKeys<NotificationsConfig>;

/**
 * A Settings mutation. Discriminated on `type` so `configReducer` is total and
 * the Settings UI can `dispatch` without knowing the update mechanics.
 */
export type ConfigAction =
  | { type: "toggleSidebarFlag"; key: SidebarBoolKey }
  | { type: "toggleNotificationsFlag"; key: NotificationsBoolKey }
  | { type: "setAppearance"; appearance: Appearance }
  // `binding: null` explicitly unbinds the action (preserved distinctly from an
  // absent key), mirroring `ShortcutsConfig.bindings`' `Option<ShortcutBinding>`.
  | { type: "setShortcutBinding"; action: string; binding: ShortcutBinding | null };

/**
 * Applies `action` to `config`, returning a new `Config`. The input is never
 * mutated. When the targeted section is absent the input is returned unchanged.
 */
export function configReducer(config: Config, action: ConfigAction): Config {
  switch (action.type) {
    case "toggleSidebarFlag": {
      const sidebar = config.sidebar;
      if (!sidebar) {
        return config;
      }
      const nextSidebar: SidebarConfig = { ...sidebar };
      nextSidebar[action.key] = !sidebar[action.key];
      return { ...config, sidebar: nextSidebar };
    }
    case "toggleNotificationsFlag": {
      const notifications = config.notifications;
      if (!notifications) {
        return config;
      }
      const nextNotifications: NotificationsConfig = { ...notifications };
      nextNotifications[action.key] = !notifications[action.key];
      return { ...config, notifications: nextNotifications };
    }
    case "setAppearance": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, appearance: action.appearance } };
    }
    case "setShortcutBinding": {
      const shortcuts = config.shortcuts;
      if (!shortcuts) {
        return config;
      }
      return {
        ...config,
        shortcuts: {
          ...shortcuts,
          bindings: { ...shortcuts.bindings, [action.action]: action.binding },
        },
      };
    }
  }
}
