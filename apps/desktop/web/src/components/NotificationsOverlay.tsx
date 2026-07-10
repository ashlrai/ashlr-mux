import { useEffect, useState } from "react";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import { useSession } from "../hooks/useSession";
import { focusedPaneStore } from "../session/focusedPane";
import { workspaceDisplayName } from "../palette/switcherEntries";
import {
  clearAllNotifications,
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  markNotificationUnread,
  removeNotification,
  type NotificationCenterItem,
  type NotificationCenterReply,
} from "../host/notifications";

export interface NotificationItem {
  id: string;
  workspaceIndex: number;
  workspaceTitle: string;
  panelId: string;
  panelTitle: string;
  unreadAt?: number;
}

interface NotificationContextMenuState {
  id: string;
  isRead: boolean;
  title: string;
  x: number;
  y: number;
}

export type NotificationContextMenuAction = "mark-read" | "mark-unread";

export interface NotificationContextMenuItem {
  action: NotificationContextMenuAction;
  disabled: boolean;
  label: string;
}

export function notificationContextMenuItems(
  isRead: boolean,
): NotificationContextMenuItem[] {
  return [
    {
      action: "mark-read",
      disabled: isRead,
      label: "Mark read",
    },
    {
      action: "mark-unread",
      disabled: !isRead,
      label: "Mark unread",
    },
  ];
}

export function notificationItems(
  workspaces: readonly SessionWorkspaceSnapshot[],
): NotificationItem[] {
  const items: NotificationItem[] = [];
  workspaces.forEach((workspace, workspaceIndex) => {
    const workspaceTitle = workspaceDisplayName(workspace);
    for (const unread of workspace.panel_unreads ?? []) {
      if (!unread.is_unread) {
        continue;
      }
      const panelTitle =
        workspace.panel_titles?.find((entry) => entry.panel_id === unread.panel_id)
          ?.custom_title ?? unread.panel_id;
      items.push({
        id: `${workspace.workspace_id ?? workspaceIndex}:${unread.panel_id}`,
        workspaceIndex,
        workspaceTitle,
        panelId: unread.panel_id,
        panelTitle,
        unreadAt: unread.unread_at,
      });
    }
  });
  return items.sort((left, right) => {
    if (left.unreadAt !== undefined && right.unreadAt !== undefined) {
      return right.unreadAt - left.unreadAt;
    }
    if (left.unreadAt !== undefined) {
      return -1;
    }
    if (right.unreadAt !== undefined) {
      return 1;
    }
    return 0;
  });
}

function unreadTimeLabel(unreadAt: number): string {
  return new Date(unreadAt * 1000).toLocaleString();
}

function notificationTimeLabel(createdAt: number): string {
  return new Date(createdAt * 1000).toLocaleString();
}

export interface NotificationsOverlayProps {
  open: boolean;
  onClose: () => void;
}

export function NotificationsOverlay({
  open,
  onClose,
}: NotificationsOverlayProps): React.JSX.Element | null {
  const { workspaces, selectWorkspace } = useSession();
  const items = notificationItems(workspaces);
  const [center, setCenter] = useState<NotificationCenterReply | null>(null);
  const [centerError, setCenterError] = useState<string | null>(null);
  const [contextMenu, setContextMenu] =
    useState<NotificationContextMenuState | null>(null);
  const [loadingCenter, setLoadingCenter] = useState(false);

  const loadCenter = () => {
    setLoadingCenter(true);
    setCenterError(null);
    void listNotifications()
      .then(setCenter)
      .catch((error) =>
        setCenterError(error instanceof Error ? error.message : String(error)),
      )
      .finally(() => setLoadingCenter(false));
  };

  const applyCenterAction = (
    action: () => Promise<NotificationCenterReply>,
  ): void => {
    setCenterError(null);
    void action()
      .then(setCenter)
      .catch((error) =>
        setCenterError(error instanceof Error ? error.message : String(error)),
      );
  };

  useEffect(() => {
    if (!open) {
      setContextMenu(null);
      return;
    }
    loadCenter();
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (contextMenu !== null) {
          setContextMenu(null);
          return;
        }
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [contextMenu, onClose, open]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="cmux-notifications-overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          setContextMenu(null);
          onClose();
        }
      }}
    >
      <section
        className="cmux-notifications-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Notifications"
        onMouseDown={(event) => {
          event.stopPropagation();
          if (
            event.target instanceof HTMLElement &&
            event.target.closest(".cmux-notifications-context-menu") == null
          ) {
            setContextMenu(null);
          }
        }}
      >
        <header className="cmux-notifications-header">
          <div>
            <h2 className="cmux-notifications-title">Notifications</h2>
            <p className="cmux-notifications-subtitle">
              {center == null
                ? "Current unread workspaces, panels, and delivered notifications."
                : `${center.unreadCount} unread of ${center.totalCount} delivered notifications.`}
            </p>
          </div>
          <div className="cmux-notifications-actions">
            <button
              type="button"
              className="cmux-notifications-close"
              disabled={loadingCenter}
              onClick={loadCenter}
            >
              Refresh
            </button>
            <button
              type="button"
              className="cmux-notifications-close"
              disabled={(center?.unreadCount ?? 0) === 0}
              onClick={() => applyCenterAction(markAllNotificationsRead)}
            >
              Mark all read
            </button>
            <button
              type="button"
              className="cmux-notifications-close"
              disabled={(center?.totalCount ?? 0) === 0}
              onClick={() => applyCenterAction(clearAllNotifications)}
            >
              Clear all
            </button>
          </div>
          <button
            type="button"
            className="cmux-notifications-close"
            onClick={onClose}
          >
            Close
          </button>
        </header>
        {centerError !== null ? (
          <div className="cmux-notifications-error">{centerError}</div>
        ) : null}
        {center?.notifications.length ? (
          <NotificationCenterList
            notifications={center.notifications}
            workspaces={workspaces}
            selectWorkspace={selectWorkspace}
            onClose={onClose}
            onMarkRead={(id) =>
              applyCenterAction(() => markNotificationRead(id))
            }
            onMarkUnread={(id) =>
              applyCenterAction(() => markNotificationUnread(id))
            }
            onRemove={(id) => applyCenterAction(() => removeNotification(id))}
            onOpenContextMenu={setContextMenu}
          />
        ) : null}
        {items.length === 0 && (center?.notifications.length ?? 0) === 0 ? (
          <div className="cmux-notifications-empty">No unread notifications.</div>
        ) : (
          <ul className="cmux-notifications-list">
            {items.map((item) => (
              <li key={item.id} className="cmux-notifications-item">
                <button
                  type="button"
                  className="cmux-notifications-jump"
                  onClick={() => {
                    focusedPaneStore.focus(item.panelId);
                    selectWorkspace(item.workspaceIndex);
                    onClose();
                  }}
                >
                  <span className="cmux-notifications-dot" aria-hidden="true" />
                  <span className="cmux-notifications-item-main">
                    <span className="cmux-notifications-item-title">
                      {item.workspaceTitle}
                    </span>
                    <span className="cmux-notifications-item-subtitle">
                      {item.panelTitle}
                    </span>
                    {item.unreadAt !== undefined ? (
                      <span className="cmux-notifications-item-time">
                        {unreadTimeLabel(item.unreadAt)}
                      </span>
                    ) : null}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {contextMenu !== null ? (
          <NotificationContextMenu
            menu={contextMenu}
            onMarkRead={(id) => {
              setContextMenu(null);
              applyCenterAction(() => markNotificationRead(id));
            }}
            onMarkUnread={(id) => {
              setContextMenu(null);
              applyCenterAction(() => markNotificationUnread(id));
            }}
          />
        ) : null}
      </section>
    </div>
  );
}

function NotificationCenterList({
  notifications,
  workspaces,
  selectWorkspace,
  onClose,
  onMarkRead,
  onMarkUnread,
  onRemove,
  onOpenContextMenu,
}: {
  notifications: readonly NotificationCenterItem[];
  workspaces: readonly SessionWorkspaceSnapshot[];
  selectWorkspace: (index: number) => void;
  onClose: () => void;
  onMarkRead: (id: string) => void;
  onMarkUnread: (id: string) => void;
  onRemove: (id: string) => void;
  onOpenContextMenu: (menu: NotificationContextMenuState) => void;
}): React.JSX.Element {
  return (
    <section className="cmux-notifications-center">
      <h3 className="cmux-notifications-section-title">Delivered</h3>
      <ul className="cmux-notifications-list">
        {notifications.map((notification) => {
          const workspaceIndex = workspaces.findIndex(
            (workspace) => workspace.workspace_id === notification.workspaceId,
          );
          const panelId = notification.panelId ?? notification.surfaceId ?? undefined;
          return (
            <li
              key={notification.id}
              className={
                notification.isRead
                  ? "cmux-notifications-item is-read"
                  : "cmux-notifications-item"
              }
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                onOpenContextMenu({
                  id: notification.id,
                  isRead: notification.isRead,
                  title: notification.title,
                  x: event.clientX,
                  y: event.clientY,
                });
              }}
            >
              <div className="cmux-notifications-center-row">
                <button
                  type="button"
                  className="cmux-notifications-jump"
                  onClick={() => {
                    if (panelId != null) {
                      focusedPaneStore.focus(panelId);
                    }
                    if (workspaceIndex >= 0) {
                      selectWorkspace(workspaceIndex);
                    }
                    onClose();
                  }}
                >
                  <span className="cmux-notifications-dot" aria-hidden="true" />
                  <span className="cmux-notifications-item-main">
                    <span className="cmux-notifications-item-title">
                      {notification.title}
                    </span>
                    <span className="cmux-notifications-item-subtitle">
                      {notification.subtitle || notification.body || notification.workspaceId}
                    </span>
                    <span className="cmux-notifications-item-time">
                      {notificationTimeLabel(notification.createdAt)}
                    </span>
                  </span>
                </button>
                <div className="cmux-notifications-row-actions">
                  <button
                    type="button"
                    disabled={notification.isRead}
                    onClick={() => onMarkRead(notification.id)}
                  >
                    Mark read
                  </button>
                  <button
                    type="button"
                    disabled={!notification.isRead}
                    onClick={() => onMarkUnread(notification.id)}
                  >
                    Mark unread
                  </button>
                  <button type="button" onClick={() => onRemove(notification.id)}>
                    Remove
                  </button>
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function NotificationContextMenu({
  menu,
  onMarkRead,
  onMarkUnread,
}: {
  menu: NotificationContextMenuState;
  onMarkRead: (id: string) => void;
  onMarkUnread: (id: string) => void;
}): React.JSX.Element {
  return (
    <div
      className="cmux-notifications-context-menu"
      role="menu"
      aria-label={`Actions for ${menu.title}`}
      style={{ left: menu.x, top: menu.y }}
      onContextMenu={(event) => event.preventDefault()}
      onMouseDown={(event) => event.stopPropagation()}
    >
      {notificationContextMenuItems(menu.isRead).map((item) => (
        <button
          key={item.action}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          onClick={() => {
            if (item.action === "mark-read") {
              onMarkRead(menu.id);
              return;
            }
            onMarkUnread(menu.id);
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
