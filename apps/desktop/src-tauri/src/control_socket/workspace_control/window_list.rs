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

pub(crate) fn workspace_list_with_recoverable_active(
    app: &AppHandle,
    session: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let active_window_id = control_active_window_id(app);
    let recoverable = app.state::<ControlClosedWindowHistoryState>().snapshots();
    let projected =
        recoverable_active_snapshot(session, params, active_window_id.as_deref(), &recoverable);
    workspace_list_from_params_for_app(app, projected.as_ref().unwrap_or(session), params)
}

fn recoverable_active_snapshot(
    session: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    active_window_id: Option<&str>,
    recoverable: &[SessionWindowSnapshot],
) -> Option<AppSessionSnapshot> {
    const ROUTING_KEYS: [&str; 9] = [
        "window_id",
        "window_ref",
        "group_id",
        "workspace_id",
        "workspace_ref",
        "surface_id",
        "terminal_id",
        "tab_id",
        "pane_id",
    ];
    if ROUTING_KEYS
        .iter()
        .any(|key| params.get(*key).is_some_and(|value| !value.is_null()))
    {
        return None;
    }
    let active_window_id = active_window_id?;
    if session
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == Some(active_window_id))
    {
        return None;
    }
    let window = recoverable
        .iter()
        .find(|window| window.window_id.as_deref() == Some(active_window_id))?;
    let mut projected = session.clone();
    projected.windows = vec![window.clone()];
    Some(projected)
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
            })
            .or_else(|| {
                recoverable.iter().find(|candidate| {
                    candidate.window_id.as_deref() == Some(window.identity.id.as_str())
                })
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
    fn live_session_payload_and_visibility_win_over_duplicate_history() {
        let live_window = session_window("same-id", "live-workspace");
        let recoverable = session_window("same-id", "closed-workspace");
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
            is_key: false,
            is_visible: true,
        }];
        let rows = window_list_rows(live, &session, &[recoverable], |kind, id| {
            format!("{kind}:{id}")
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["visible"], true);
        assert_eq!(rows[0]["selected_workspace_id"], "live-workspace");
    }

    #[test]
    fn lingering_native_row_uses_recoverable_payload() {
        let closed = session_window("closed-id", "closed-workspace");
        let session = AppSessionSnapshot {
            windows: vec![session_window("live-id", "live-workspace")],
            ..Default::default()
        };
        let live = vec![crate::window::WindowControlSummary {
            identity: WindowControlIdentity {
                label: "closed-id".into(),
                id: "closed-id".into(),
                reference: "window:closed-id".into(),
            },
            is_key: false,
            is_visible: false,
        }];
        let rows = window_list_rows(live, &session, &[closed], |kind, id| format!("{kind}:{id}"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["workspace_count"], 1);
        assert_eq!(rows[0]["selected_workspace_id"], "closed-workspace");
        assert_eq!(rows[0]["visible"], false);
    }

    #[test]
    fn selectorless_workspace_list_projects_the_recoverable_active_manager() {
        let live = session_window("live-id", "live-workspace");
        let closed = session_window("closed-id", "closed-workspace");
        let session = AppSessionSnapshot {
            windows: vec![live],
            ..Default::default()
        };
        let empty = serde_json::Map::new();

        let projected = recoverable_active_snapshot(
            &session,
            &empty,
            Some("closed-id"),
            std::slice::from_ref(&closed),
        )
        .expect("closed active manager remains routable");
        assert_eq!(projected.windows, [closed]);

        assert!(
            recoverable_active_snapshot(&session, &empty, Some("live-id"), &projected.windows,)
                .is_none()
        );
        let explicit = serde_json::Map::from_iter([("window_id".into(), json!("live-id"))]);
        assert!(recoverable_active_snapshot(
            &session,
            &explicit,
            Some("closed-id"),
            &projected.windows,
        )
        .is_none());
    }
}
