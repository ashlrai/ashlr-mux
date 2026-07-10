import type { NotificationsConfig } from "@cmux/core-types";

import { DEFAULT_NOTIFICATIONS_CONFIG } from "./defaultConfig";

export interface NotificationDeliveryEffects {
  desktop: boolean;
  soundEffect: boolean;
  commandEffect: boolean;
  suppressed: boolean;
}

export interface NotificationDeliverySettingsRequest extends NotificationDeliveryEffects {
  sound: string;
  customSoundFilePath: string;
}

function notificationConfig(
  notifications: NotificationsConfig | null | undefined,
): NotificationsConfig {
  return notifications ?? DEFAULT_NOTIFICATIONS_CONFIG;
}

export function normalizedNotificationCustomSoundPath(
  path: string | null | undefined,
): string | null {
  const trimmed = path?.trim();
  return trimmed === "" || trimmed == null ? null : trimmed;
}

export function notificationSoundEffectEnabled(
  notifications: NotificationsConfig | null | undefined,
): boolean {
  const config = notificationConfig(notifications);
  if (config.sound === "none") {
    return false;
  }
  if (config.sound === "custom_file") {
    return normalizedNotificationCustomSoundPath(config.customSoundFilePath) != null;
  }
  return true;
}

export function notificationDeliverySettingsRequest(
  notifications: NotificationsConfig | null | undefined,
): NotificationDeliverySettingsRequest {
  const config = notificationConfig(notifications);
  return {
    sound: config.sound,
    customSoundFilePath: config.customSoundFilePath,
    desktop: true,
    soundEffect: notificationSoundEffectEnabled(config),
    commandEffect: config.command.trim() !== "",
    suppressed: false,
  };
}
