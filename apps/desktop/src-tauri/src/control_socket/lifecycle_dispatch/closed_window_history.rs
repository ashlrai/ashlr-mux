use std::collections::HashMap;
use std::sync::Mutex;

use cmux_core::session::SessionWindowSnapshot;

/// In-process mirror of canonical recoverable main-window routes.
///
/// Closing a native window removes it from the live session snapshot, while
/// canonical keeps its complete tab manager reachable for recently-closed
/// history and appends that invisible route to `window.list` until restart.
#[derive(Default)]
pub(crate) struct ControlClosedWindowHistoryState {
    inner: Mutex<ClosedWindowHistory>,
}

#[derive(Default)]
struct ClosedWindowHistory {
    committed: Vec<SessionWindowSnapshot>,
    pending: HashMap<String, SessionWindowSnapshot>,
}

impl ControlClosedWindowHistoryState {
    pub(crate) fn stage(&self, window: SessionWindowSnapshot) {
        let Some(window_id) = window.window_id.clone() else {
            return;
        };
        self.inner
            .lock()
            .expect("closed-window history poisoned")
            .pending
            .insert(window_id, window);
    }

    pub(crate) fn commit(&self, window_id: &str) {
        let mut inner = self.inner.lock().expect("closed-window history poisoned");
        let Some(window) = inner.pending.remove(window_id) else {
            return;
        };
        inner
            .committed
            .retain(|entry| entry.window_id.as_deref() != Some(window_id));
        inner.committed.insert(0, window);
    }

    pub(crate) fn discard(&self, window_id: &str) {
        self.inner
            .lock()
            .expect("closed-window history poisoned")
            .pending
            .remove(window_id);
    }

    pub(crate) fn contains(&self, window_id: &str) -> bool {
        self.inner
            .lock()
            .expect("closed-window history poisoned")
            .committed
            .iter()
            .any(|window| window.window_id.as_deref() == Some(window_id))
    }

    pub(crate) fn snapshots(&self) -> Vec<SessionWindowSnapshot> {
        self.inner
            .lock()
            .expect("closed-window history poisoned")
            .committed
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::recoverable_window_close_id;
    use super::*;
    use cmux_core::session::AppSessionSnapshot;
    use serde_json::json;

    fn window(id: &str) -> SessionWindowSnapshot {
        SessionWindowSnapshot {
            window_id: Some(id.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn records_newest_first_and_replaces_duplicate_identity() {
        let history = ControlClosedWindowHistoryState::default();
        for id in ["window-a", "window-b", "window-a"] {
            history.stage(window(id));
            history.commit(id);
        }

        let ids: Vec<_> = history
            .snapshots()
            .into_iter()
            .filter_map(|window| window.window_id)
            .collect();
        assert_eq!(ids, ["window-a", "window-b"]);
    }

    #[test]
    fn ignores_identityless_windows() {
        let history = ControlClosedWindowHistoryState::default();
        history.stage(SessionWindowSnapshot::default());
        assert!(history.snapshots().is_empty());
    }

    #[test]
    fn pending_history_is_invisible_and_can_be_discarded() {
        let history = ControlClosedWindowHistoryState::default();
        history.stage(window("window-a"));
        assert!(!history.contains("window-a"));
        assert!(history.snapshots().is_empty());
        history.discard("window-a");
        history.commit("window-a");
        assert!(!history.contains("window-a"));
        assert!(history.snapshots().is_empty());
    }

    #[test]
    fn repeat_close_resolves_only_committed_recoverable_windows() {
        let current = AppSessionSnapshot {
            windows: vec![window("window-a"), window("window-b")],
            ..Default::default()
        };
        let history = ControlClosedWindowHistoryState::default();
        history.stage(current.windows[1].clone());
        let params = serde_json::Map::from_iter([("window_id".into(), json!("window-b"))]);

        assert_eq!(
            recoverable_window_close_id(&current, "window.close", &params, &history),
            None
        );
        history.commit("window-b");
        assert_eq!(
            recoverable_window_close_id(&current, "window.close", &params, &history),
            None,
            "a live window must still use the normal transition"
        );

        let mut after_close = current;
        after_close.windows.pop();
        assert_eq!(
            recoverable_window_close_id(&after_close, "window.close", &params, &history).as_deref(),
            Some("window-b")
        );
        assert_eq!(
            recoverable_window_close_id(&after_close, "window.focus", &params, &history),
            None
        );
    }
}
