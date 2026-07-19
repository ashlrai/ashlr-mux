//! Canonical routing regressions for workspace.next / previous.
//!
//! The pinned macOS implementation resolves a TabManager from the full v2
//! routing selectors before navigating. These tests keep two windows alive so
//! accidentally navigating `snapshot.windows.first()` is observable.

use super::*;

fn navigation_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("window-a".into());
    snapshot.windows[0].selected_workspace_id = Some("workspace-a2".into());
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(1);
    let template = snapshot.windows[0].tab_manager.workspaces[0].clone();
    snapshot.windows[0].tab_manager.workspaces = (1..=3)
        .map(|index| {
            let mut workspace = template.clone();
            workspace.workspace_id = Some(format!("workspace-a{index}"));
            workspace
        })
        .collect();

    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("window-b".into());
    second.selected_workspace_id = Some("workspace-b3".into());
    second.tab_manager.selected_workspace_index = Some(2);
    second.tab_manager.workspaces = (1..=3)
        .map(|index| {
            let mut workspace = template.clone();
            workspace.workspace_id = Some(format!("workspace-b{index}"));
            workspace
        })
        .collect();
    snapshot.windows.push(second);
    snapshot
}

#[test]
fn workspace_next_routes_to_explicit_window_and_wraps() {
    let snapshot = navigation_snapshot();
    let params = serde_json::Map::from_iter([("window_id".into(), json!("window-b"))]);

    let target = workspace_relative_target(&snapshot, &params, Some("window-a"), 1)
        .expect("explicit window must resolve");

    assert_eq!(target.window_index, 1);
    assert_eq!(target.workspace_index, 0);
    assert_eq!(target.workspace_id, "workspace-b1");
}

#[test]
fn workspace_previous_routes_by_workspace_owner_before_active_window() {
    let snapshot = navigation_snapshot();
    let params =
        serde_json::Map::from_iter([("workspace_id".into(), json!("workspace-b1"))]);

    let target = workspace_relative_target(&snapshot, &params, Some("window-a"), -1)
        .expect("workspace owner must resolve");

    assert_eq!(target.window_index, 1);
    assert_eq!(target.workspace_index, 1);
    assert_eq!(target.workspace_id, "workspace-b2");
}

#[test]
fn workspace_navigation_uses_active_window_without_selectors() {
    let snapshot = navigation_snapshot();
    let target = workspace_relative_target(
        &snapshot,
        &serde_json::Map::new(),
        Some("window-b"),
        1,
    )
    .expect("active window must resolve");

    assert_eq!(target.window_index, 1);
    assert_eq!(target.workspace_id, "workspace-b1");
}

#[test]
fn workspace_navigation_distinguishes_unavailable_from_no_selection() {
    let mut snapshot = navigation_snapshot();
    let invalid =
        serde_json::Map::from_iter([("window_id".into(), json!("missing-window"))]);
    assert_eq!(
        workspace_relative_target(&snapshot, &invalid, Some("window-a"), 1),
        Err(WorkspaceNavigationTargetError::TabManagerUnavailable)
    );

    snapshot.windows[1].tab_manager.selected_workspace_index = None;
    snapshot.windows[1].selected_workspace_id = None;
    assert_eq!(
        workspace_relative_target(
            &snapshot,
            &serde_json::Map::new(),
            Some("window-b"),
            1,
        ),
        Err(WorkspaceNavigationTargetError::NoWorkspaceSelected)
    );
}
