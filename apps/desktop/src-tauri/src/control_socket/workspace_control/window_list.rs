use std::collections::HashSet;

use cmux_core::session::{AppSessionSnapshot, SessionWindowSnapshot};
use serde_json::{json, Value};

use super::*;

pub(crate) fn window_list(app: &AppHandle) -> ControlCallResult {
    let session = snapshot(app);
    let live = crate::window::control_window_summaries(app);
    let recoverable = app.state::<ControlClosedWindowHistoryState>().snapshots();
    let rows = window_list_rows(live, &session, &recoverable, |kind, id| {
        control_handle_ref(app, kind, id)
    });
    ok(json!({"windows": rows}))
}

fn window_list_rows(
    live: Vec<crate::window::WindowControlSummary>,
    session: &AppSessionSnapshot,
    recoverable: &[SessionWindowSnapshot],
    mut handle_ref: impl FnMut(&'static str, &str) -> String,
) -> Vec<Value> {
    let mut listed_ids = HashSet::new();
    let mut rows = Vec::with_capacity(live.len() + recoverable.len());
    for window in live {
        let session_window = session
            .windows
            .iter()
            .find(|candidate| {
                candidate.window_id.as_deref() == Some(window.identity.label.as_str())
            })
            .or_else(|| {
                (window.identity.label == "main")
                    .then(|| session.windows.first())
                    .flatten()
            });
        let window_id = session_window
            .and_then(|window| window.window_id.as_deref())
            .unwrap_or(window.identity.id.as_str());
        listed_ids.insert(window_id.to_string());
        rows.push(window_row(
            rows.len(),
            window_id,
            session_window,
            window.is_key,
            window.is_visible,
            &mut handle_ref,
        ));
    }
    for window in recoverable {
        let Some(window_id) = window.window_id.as_deref() else {
            continue;
        };
        if !listed_ids.insert(window_id.to_string()) {
            continue;
        }
        rows.push(window_row(
            rows.len(),
            window_id,
            Some(window),
            false,
            false,
            &mut handle_ref,
        ));
    }
    rows
}

fn window_row(
    index: usize,
    window_id: &str,
    window: Option<&SessionWindowSnapshot>,
    key: bool,
    visible: bool,
    handle_ref: &mut impl FnMut(&'static str, &str) -> String,
) -> Value {
    let tab_manager = window.map(|window| &window.tab_manager);
    let selected_index = tab_manager
        .and_then(|manager| manager.selected_workspace_index)
        .unwrap_or_default()
        .max(0) as usize;
    let selected_workspace_id = tab_manager
        .and_then(|manager| manager.workspaces.get(selected_index))
        .and_then(|workspace| workspace.workspace_id.clone());
    let selected_workspace_ref = selected_workspace_id
        .as_deref()
        .map(|id| handle_ref("workspace", id));
    json!({
        "index": index,
        "id": window_id,
        "ref": handle_ref("window", window_id),
        "key": key,
        "visible": visible,
        "workspace_count": tab_manager.map_or(0, |manager| manager.workspaces.len()),
        "selected_workspace_id": selected_workspace_id,
        "selected_workspace_ref": selected_workspace_ref,
    })
}

#[cfg(test)]
mod tests {
    use cmux_core::session::{SessionTabManagerSnapshot, SessionWorkspaceSnapshot};
    use cmux_core::window_display::WindowControlIdentity;

    use super::*;

    fn session_window(id: &str, workspace_id: &str) -> SessionWindowSnapshot {
        SessionWindowSnapshot {
            window_id: Some(id.to_string()),
            selected_workspace_id: Some(workspace_id.to_string()),
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![SessionWorkspaceSnapshot {
                    workspace_id: Some(workspace_id.to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn appends_recoverable_rows_after_live_rows_with_strict_state() {
        let live_window = session_window("live-id", "live-workspace");
        let closed_window = session_window("closed-id", "closed-workspace");
        let session = AppSessionSnapshot {
            windows: vec![live_window],
            ..Default::default()
        };
        let live = vec![crate::window::WindowControlSummary {
            identity: WindowControlIdentity {
                label: "main".into(),
                id: "window-1".into(),
                reference: "window:1".into(),
            },
            is_key: true,
            is_visible: true,
        }];

        let rows = window_list_rows(live, &session, &[closed_window], |kind, id| {
            format!("{kind}:{id}")
        });

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "live-id");
        assert_eq!(rows[0]["key"], true);
        assert_eq!(rows[0]["visible"], true);
        assert_eq!(rows[1]["id"], "closed-id");
        assert_eq!(rows[1]["index"], 1);
        assert_eq!(rows[1]["key"], false);
        assert_eq!(rows[1]["visible"], false);
        assert_eq!(rows[1]["workspace_count"], 1);
        assert_eq!(rows[1]["selected_workspace_id"], "closed-workspace");
        assert_eq!(rows[1]["ref"], "window:closed-id");
        assert_eq!(
            rows[1]["selected_workspace_ref"],
            "workspace:closed-workspace"
        );
    }

    #[test]
    fn live_identity_wins_over_duplicate_history_entry() {
        let window = session_window("same-id", "workspace-id");
        let session = AppSessionSnapshot {
            windows: vec![window.clone()],
            ..Default::default()
        };
        let live = vec![crate::window::WindowControlSummary {
            identity: WindowControlIdentity {
                label: "main".into(),
                id: "window-1".into(),
                reference: "window:1".into(),
            },
            is_key: false,
            is_visible: true,
        }];
        let rows = window_list_rows(live, &session, &[window], |kind, id| format!("{kind}:{id}"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["visible"], true);
    }
}
