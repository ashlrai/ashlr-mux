import { describe, expect, mock, test } from "bun:test";

const invokeCalls: Array<{ method: string; params: unknown }> = [];

mock.module("./host", () => ({
  host: {
    invoke: async (method: string, params?: unknown) => {
      invokeCalls.push({ method, params });
      return { notifications: [], unreadCount: 0, totalCount: 0 };
    },
  },
}));

const {
  clearAllNotifications,
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  markNotificationUnread,
  recordWaitingInputNotification,
  removeNotification,
} = await import("./notifications");

describe("notification center host adapter", () => {
  test("lists notifications", async () => {
    invokeCalls.length = 0;
    await listNotifications();
    expect(invokeCalls).toEqual([
      { method: "notification_list", params: undefined },
    ]);
  });

  test("marks one notification read", async () => {
    invokeCalls.length = 0;
    await markNotificationRead("n-1");
    expect(invokeCalls).toEqual([
      { method: "notification_mark_read", params: { id: "n-1" } },
    ]);
  });

  test("marks one notification unread", async () => {
    invokeCalls.length = 0;
    await markNotificationUnread("n-1");
    expect(invokeCalls).toEqual([
      { method: "notification_mark_unread", params: { id: "n-1" } },
    ]);
  });

  test("removes one notification", async () => {
    invokeCalls.length = 0;
    await removeNotification("n-1");
    expect(invokeCalls).toEqual([
      { method: "notification_remove", params: { id: "n-1" } },
    ]);
  });

  test("marks all notifications read", async () => {
    invokeCalls.length = 0;
    await markAllNotificationsRead();
    expect(invokeCalls).toEqual([
      { method: "notification_mark_all_read", params: undefined },
    ]);
  });

  test("clears all notifications", async () => {
    invokeCalls.length = 0;
    await clearAllNotifications();
    expect(invokeCalls).toEqual([
      { method: "notification_clear_all", params: undefined },
    ]);
  });

  test("records an agent waiting-input notification", async () => {
    invokeCalls.length = 0;
    await recordWaitingInputNotification({
      workspaceId: "workspace-1",
      panelId: "panel-1",
      workspaceTitle: "Phoenix",
      panelTitle: "API logs",
    });
    expect(invokeCalls).toEqual([
      {
        method: "notification_record_waiting_input",
        params: {
          workspaceId: "workspace-1",
          panelId: "panel-1",
          workspaceTitle: "Phoenix",
          panelTitle: "API logs",
        },
      },
    ]);
  });
});
