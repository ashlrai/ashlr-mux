import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import {
  NotificationsOverlay,
  notificationContextMenuItems,
  notificationItems,
} from "./NotificationsOverlay";

function workspace(
  title: string,
  unreadPanelIds: readonly string[] = [],
  unreadAtByPanelId: Readonly<Record<string, number>> = {},
): SessionWorkspaceSnapshot {
  return {
    workspace_id: title,
    process_title: title,
    layout: {
      type: "pane",
      pane: {
        panel_ids: [...unreadPanelIds],
        selected_panel_id: unreadPanelIds[0],
      },
    },
    panel_titles: unreadPanelIds.map((panel_id) => ({
      panel_id,
      custom_title: `Panel ${panel_id}`,
    })),
    panel_unreads: unreadPanelIds.map((panel_id) => ({
      panel_id,
      is_unread: true,
      unread_at: unreadAtByPanelId[panel_id],
    })),
  };
}

describe("notificationItems", () => {
  test("projects unread panel metadata into notification rows", () => {
    expect(notificationItems([workspace("Phoenix", ["surface-1"])])).toEqual([
      {
        id: "Phoenix:surface-1",
        workspaceIndex: 0,
        workspaceTitle: "Phoenix",
        panelId: "surface-1",
        panelTitle: "Panel surface-1",
        unreadAt: undefined,
      },
    ]);
  });

  test("sorts timestamped notifications newest first", () => {
    expect(
      notificationItems([
        workspace("Phoenix", ["surface-1"], { "surface-1": 10 }),
        workspace("Orion", ["surface-2"], { "surface-2": 30 }),
        workspace("Legacy", ["surface-3"]),
      ]).map((item) => item.id),
    ).toEqual(["Orion:surface-2", "Phoenix:surface-1", "Legacy:surface-3"]);
  });

  test("ignores read panel metadata", () => {
    const ws = workspace("Phoenix", ["surface-1"]);
    ws.panel_unreads = [{ panel_id: "surface-1", is_unread: false }];
    expect(notificationItems([ws])).toEqual([]);
  });
});

describe("NotificationsOverlay", () => {
  test("renders nothing while closed", () => {
    expect(renderToStaticMarkup(<NotificationsOverlay open={false} onClose={() => {}} />)).toBe("");
  });

  test("renders backend notification actions while open", () => {
    const markup = renderToStaticMarkup(
      <NotificationsOverlay open={true} onClose={() => {}} />,
    );

    expect(markup).toContain("Refresh");
    expect(markup).toContain("Mark all read");
    expect(markup).toContain("Clear all");
    expect(markup).toContain("No unread notifications.");
  });

  test("describes right-click read/unread context menu state", () => {
    expect(notificationContextMenuItems(false)).toEqual([
      {
        action: "mark-read",
        disabled: false,
        label: "Mark read",
      },
      {
        action: "mark-unread",
        disabled: true,
        label: "Mark unread",
      },
    ]);

    expect(notificationContextMenuItems(true)).toEqual([
      {
        action: "mark-read",
        disabled: true,
        label: "Mark read",
      },
      {
        action: "mark-unread",
        disabled: false,
        label: "Mark unread",
      },
    ]);
  });

  test("styles notification buttons with outside focus outlines", async () => {
    const css = await Bun.file(new URL("../styles.css", import.meta.url)).text();

    expect(css).toContain(".cmux-notifications-close:focus-visible");
    expect(css).toContain(".cmux-notifications-jump:focus-visible");
    expect(css).toContain(
      ".cmux-notifications-context-menu button:not(:disabled):focus-visible",
    );
    expect(css).toContain(
      ".cmux-notifications-row-actions button:not(:disabled):focus-visible",
    );
    expect(css).toContain("outline-offset: 2px");
  });

  test("styles the notification context menu", async () => {
    const css = await Bun.file(new URL("../styles.css", import.meta.url)).text();

    expect(css).toContain(".cmux-notifications-context-menu");
    expect(css).toContain("position: fixed");
    expect(css).toContain(
      ".cmux-notifications-context-menu button:not(:disabled):hover",
    );
  });
});
