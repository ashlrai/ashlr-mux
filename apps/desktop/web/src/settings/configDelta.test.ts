import { describe, expect, test } from "bun:test";

import type { Config } from "@cmux/core-types";

import { withSettingsDefaults } from "./configDefaults";
import { deltaForAction } from "./configDelta";
import { configReducer } from "./configReducer";

const effective = withSettingsDefaults({});

describe("deltaForAction", () => {
  test("sidebar toggle flips off the effective value", () => {
    // showSSH defaults true → toggling writes false.
    expect(
      deltaForAction(effective, { type: "toggleSidebarFlag", key: "showSSH" }),
    ).toEqual({ kind: "set", path: "sidebar.showSSH", value: false });
    // hideAllDetails defaults false → toggling writes true.
    expect(
      deltaForAction(effective, {
        type: "toggleSidebarFlag",
        key: "hideAllDetails",
      }),
    ).toEqual({ kind: "set", path: "sidebar.hideAllDetails", value: true });
  });

  test("notifications toggle and appearance map to their dotted paths", () => {
    expect(
      deltaForAction(effective, {
        type: "toggleNotificationsFlag",
        key: "dockBadge",
      }),
    ).toEqual({ kind: "set", path: "notifications.dockBadge", value: false });
    expect(
      deltaForAction(effective, { type: "setAppearance", appearance: "dark" }),
    ).toEqual({ kind: "set", path: "app.appearance", value: "dark" });
  });

  test("returns null exactly when the reducer no-ops (absent section)", () => {
    const bare: Config = {};
    const action = { type: "toggleSidebarFlag", key: "showSSH" } as const;
    expect(deltaForAction(bare, action)).toBeNull();
    expect(configReducer(bare, action)).toBe(bare); // reducer no-op too
  });

  test("shortcut binding writes the explicit value including null unbind", () => {
    const withShortcuts = { shortcuts: { bindings: {} } } as unknown as Config;
    expect(
      deltaForAction(withShortcuts, {
        type: "setShortcutBinding",
        action: "newWorkspace",
        binding: null,
      }),
    ).toEqual({
      kind: "set",
      path: "shortcuts.bindings.newWorkspace",
      value: null,
    });
    // A dotted action id would corrupt the path — guarded to null.
    expect(
      deltaForAction(withShortcuts, {
        type: "setShortcutBinding",
        action: "bad.id",
        binding: null,
      }),
    ).toBeNull();
  });

  test("delta value agrees with the reducer's next state", () => {
    const action = { type: "toggleSidebarFlag", key: "watchGitStatus" } as const;
    const delta = deltaForAction(effective, action);
    const next = configReducer(effective, action);
    expect(delta).toEqual({
      kind: "set",
      path: "sidebar.watchGitStatus",
      value: next.sidebar?.watchGitStatus,
    });
  });
});
