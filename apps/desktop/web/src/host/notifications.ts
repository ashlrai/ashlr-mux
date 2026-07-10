import { host } from "./host";

export interface NotificationCenterItem {
  id: string;
  workspaceId: string;
  surfaceId?: string | null;
  panelId?: string | null;
  title: string;
  subtitle: string;
  body: string;
  createdAt: number;
  isRead: boolean;
}

export interface NotificationCenterReply {
  notifications: NotificationCenterItem[];
  unreadCount: number;
  totalCount: number;
}

export interface WaitingInputNotificationRequest {
  workspaceId: string;
  panelId: string;
  workspaceTitle?: string | null;
  panelTitle?: string | null;
}

export function listNotifications(): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_list");
}

export function markNotificationRead(id: string): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_mark_read", { id });
}

export function markNotificationUnread(id: string): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_mark_unread", { id });
}

export function removeNotification(id: string): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_remove", { id });
}

export function markAllNotificationsRead(): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_mark_all_read");
}

export function clearAllNotifications(): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>("notification_clear_all");
}

export function recordWaitingInputNotification(
  request: WaitingInputNotificationRequest,
): Promise<NotificationCenterReply> {
  return host.invoke<NotificationCenterReply>(
    "notification_record_waiting_input",
    {
      workspaceId: request.workspaceId,
      panelId: request.panelId,
      workspaceTitle: request.workspaceTitle,
      panelTitle: request.panelTitle,
    },
  );
}
