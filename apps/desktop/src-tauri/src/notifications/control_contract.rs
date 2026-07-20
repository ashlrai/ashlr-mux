use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlNotificationSnapshot {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    pub(crate) surface_id: Option<String>,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) body: String,
    pub(crate) created_at: i64,
    pub(crate) is_read: bool,
}

impl From<&TerminalNotification> for ControlNotificationSnapshot {
    fn from(notification: &TerminalNotification) -> Self {
        Self {
            id: notification.id.clone(),
            workspace_id: notification.tab_id.clone(),
            surface_id: notification
                .surface_id
                .clone()
                .or_else(|| notification.panel_id.clone()),
            title: notification.title.clone(),
            subtitle: notification.subtitle.clone(),
            body: notification.body.clone(),
            created_at: notification.created_at,
            is_read: notification.is_read,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlNotificationDismissOutcome {
    Dismissed(ControlNotificationSnapshot),
    AllRead {
        dismissed: usize,
        removed: Vec<ControlNotificationSnapshot>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlNotificationMarkReadOutcome {
    pub(crate) marked: usize,
    pub(crate) changed: Vec<ControlNotificationSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlNotificationClearOutcome {
    pub(crate) cleared_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlNotificationOpenOutcome {
    pub(crate) notification: ControlNotificationSnapshot,
    pub(crate) marked_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlNotificationCreateOutcome {
    pub(crate) notification: ControlNotificationSnapshot,
    pub(crate) replaced: Vec<ControlNotificationSnapshot>,
}

pub(crate) fn notification_list_for_control(
    state: &NotificationCommandState,
) -> Result<Vec<ControlNotificationSnapshot>, String> {
    with_notification_store(state, |store| {
        store
            .notifications()
            .iter()
            .map(ControlNotificationSnapshot::from)
            .collect()
    })
}

pub(crate) fn notification_dismiss_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
    all_read: bool,
) -> Result<ControlNotificationDismissOutcome, String> {
    with_notification_store(state, |store| {
        if let Some(id) = id {
            let notification = store
                .notifications()
                .iter()
                .find(|item| item.id == id)
                .cloned()
                .ok_or_else(|| "Notification not found".to_owned())?;
            store.remove(id);
            return Ok(ControlNotificationDismissOutcome::Dismissed(
                ControlNotificationSnapshot::from(&notification),
            ));
        }

        let removed = store
            .notifications()
            .iter()
            .filter(|item| all_read && item.is_read)
            .map(ControlNotificationSnapshot::from)
            .collect::<Vec<_>>();
        for notification in &removed {
            store.remove(&notification.id);
        }
        Ok(ControlNotificationDismissOutcome::AllRead {
            dismissed: removed.len(),
            removed,
        })
    })?
}

pub(crate) fn notification_mark_read_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
    workspace_id: Option<&str>,
    surface_id: Option<&str>,
    all: bool,
) -> Result<ControlNotificationMarkReadOutcome, String> {
    with_notification_store(state, |store| {
        let changed_ids = if let Some(id) = id {
            if !store.notifications().iter().any(|item| item.id == id) {
                return Err("Notification not found".to_owned());
            }
            store.mark_read(id).into_iter().collect()
        } else if let Some(workspace_id) = workspace_id {
            if surface_id.is_some() {
                store.mark_read_for_tab_surface(workspace_id, surface_id)
            } else {
                store.mark_read_for_tab(workspace_id)
            }
        } else if all {
            store.mark_all_read()
        } else {
            Vec::new()
        };
        let changed = changed_ids
            .iter()
            .filter_map(|id| {
                store
                    .notifications()
                    .iter()
                    .find(|notification| notification.id == *id)
                    .map(ControlNotificationSnapshot::from)
            })
            .collect::<Vec<_>>();
        Ok(ControlNotificationMarkReadOutcome {
            marked: changed_ids.len(),
            changed,
        })
    })?
}

pub(crate) fn notification_clear_for_control(
    state: &NotificationCommandState,
    workspace_id: Option<&str>,
) -> Result<ControlNotificationClearOutcome, String> {
    with_notification_store(state, |store| {
        let cleared_ids = if let Some(workspace_id) = workspace_id {
            store.clear_for_tab(workspace_id)
        } else {
            store.clear_all()
        };
        ControlNotificationClearOutcome { cleared_ids }
    })
}

pub(crate) fn notification_open_target_for_control(
    state: &NotificationCommandState,
    id: Option<&str>,
) -> Result<Option<ControlNotificationOpenOutcome>, String> {
    with_notification_store(state, |store| {
        let target_id = match id {
            Some(id) => store
                .notifications()
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.id.clone()),
            None => store
                .notifications()
                .iter()
                .find(|item| !item.is_read)
                .map(|item| item.id.clone()),
        }?;
        let marked_read = store.mark_read(&target_id).is_some();
        store
            .notifications()
            .iter()
            .find(|item| item.id == target_id)
            .map(ControlNotificationSnapshot::from)
            .map(|notification| ControlNotificationOpenOutcome {
                notification,
                marked_read,
            })
    })
}

pub(crate) fn notification_create_for_control(
    state: &NotificationCommandState,
    workspace_id: String,
    surface_id: String,
    title: String,
    subtitle: String,
    body: String,
) -> Result<ControlNotificationCreateOutcome, String> {
    let notification = TerminalNotification {
        id: uuid::Uuid::new_v4().to_string(),
        tab_id: workspace_id,
        surface_id: Some(surface_id.clone()),
        panel_id: Some(surface_id),
        title,
        subtitle,
        body,
        created_at: current_unix_timestamp_seconds(),
        is_read: false,
        pane_flash: true,
        click_action: None,
    };
    let replaced = with_notification_store(state, |store| {
        let replaced = store
            .notifications()
            .iter()
            .filter(|existing| {
                existing.tab_id == notification.tab_id
                    && existing.surface_id == notification.surface_id
            })
            .map(ControlNotificationSnapshot::from)
            .collect::<Vec<_>>();
        store.record(notification.clone(), false);
        replaced
    })?;
    let _ = deliver_control_notification(&notification);
    Ok(ControlNotificationCreateOutcome {
        notification: ControlNotificationSnapshot::from(&notification),
        replaced,
    })
}
