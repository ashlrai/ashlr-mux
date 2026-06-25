use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalNotification {
    pub id: String,
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub created_at: i64,
    #[serde(default)]
    pub is_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotificationState {
    pub notifications: Vec<TerminalNotification>,
    pub unread_count: usize,
}

impl NotificationState {
    pub fn record(&mut self, notification: TerminalNotification) {
        self.notifications.retain(|existing| existing.id != notification.id);
        self.notifications.push(notification);
        self.notifications.sort_by(|lhs, rhs| {
            rhs.created_at
                .cmp(&lhs.created_at)
                .then_with(|| lhs.id.cmp(&rhs.id))
        });
        self.refresh();
    }

    pub fn mark_read(&mut self, id: &str) {
        if let Some(notification) = self.notifications.iter_mut().find(|item| item.id == id) {
            notification.is_read = true;
        }
        self.refresh();
    }

    pub fn mark_unread(&mut self, id: &str) {
        if let Some(notification) = self.notifications.iter_mut().find(|item| item.id == id) {
            notification.is_read = false;
        }
        self.refresh();
    }

    pub fn remove(&mut self, id: &str) {
        self.notifications.retain(|notification| notification.id != id);
        self.refresh();
    }

    pub fn clear(&mut self) {
        self.notifications.clear();
        self.refresh();
    }

    fn refresh(&mut self) {
        self.unread_count = self
            .notifications
            .iter()
            .filter(|notification| !notification.is_read)
            .count();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, created_at: i64) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: "tab".into(),
            surface_id: Some("surface".into()),
            panel_id: None,
            title: "Title".into(),
            subtitle: "Subtitle".into(),
            body: "Body".into(),
            created_at,
            is_read: false,
        }
    }

    #[test]
    fn reducer_tracks_unread_counts() {
        let mut state = NotificationState::default();
        state.record(notification("a", 1));
        state.record(notification("b", 2));
        assert_eq!(state.unread_count, 2);
        assert_eq!(state.notifications[0].id, "b");

        state.mark_read("b");
        assert_eq!(state.unread_count, 1);

        state.remove("a");
        assert_eq!(state.unread_count, 0);
    }
}
