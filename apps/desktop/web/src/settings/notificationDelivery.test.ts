import type { NotificationsConfig } from "@cmux/core-types";
import { describe, expect, test } from "bun:test";

import {
  notificationDeliverySettingsRequest,
  notificationSoundEffectEnabled,
  normalizedNotificationCustomSoundPath,
} from "./notificationDelivery";

function notifications(
  overrides: Partial<NotificationsConfig> = {},
): NotificationsConfig {
  return {
    dockBadge: true,
    showInMenuBar: true,
    unreadPaneRing: true,
    paneFlash: true,
    sound: "default",
    customSoundFilePath: "",
    command: "",
    hooksMode: "append",
    hooks: [],
    ...overrides,
  };
}

describe("normalizedNotificationCustomSoundPath", () => {
  test("trims whitespace and converts empty paths to null", () => {
    expect(normalizedNotificationCustomSoundPath(" C:/sounds/cmux.wav ")).toBe(
      "C:/sounds/cmux.wav",
    );
    expect(normalizedNotificationCustomSoundPath(" \n\t ")).toBeNull();
    expect(normalizedNotificationCustomSoundPath(undefined)).toBeNull();
  });
});

describe("notificationSoundEffectEnabled", () => {
  test("matches the Rust sound resolver's audible cases", () => {
    expect(notificationSoundEffectEnabled(notifications({ sound: "default" }))).toBe(
      true,
    );
    expect(notificationSoundEffectEnabled(notifications({ sound: "Ping" }))).toBe(true);
    expect(notificationSoundEffectEnabled(notifications({ sound: "none" }))).toBe(
      false,
    );
    expect(
      notificationSoundEffectEnabled(
        notifications({ sound: "custom_file", customSoundFilePath: "" }),
      ),
    ).toBe(false);
    expect(
      notificationSoundEffectEnabled(
        notifications({
          sound: "custom_file",
          customSoundFilePath: " C:/sounds/cmux.wav ",
        }),
      ),
    ).toBe(true);
  });
});

describe("notificationDeliverySettingsRequest", () => {
  test("projects settings into the backend preview request shape", () => {
    expect(
      notificationDeliverySettingsRequest(
        notifications({
          sound: "custom_file",
          customSoundFilePath: " C:/sounds/cmux.wav ",
          command: " echo cmux ",
        }),
      ),
    ).toEqual({
      sound: "custom_file",
      customSoundFilePath: " C:/sounds/cmux.wav ",
      desktop: true,
      soundEffect: true,
      commandEffect: true,
      suppressed: false,
    });
  });

  test("uses defaults when config has not loaded yet", () => {
    expect(notificationDeliverySettingsRequest(undefined)).toEqual({
      sound: "default",
      customSoundFilePath: "",
      desktop: true,
      soundEffect: true,
      commandEffect: false,
      suppressed: false,
    });
  });
});
