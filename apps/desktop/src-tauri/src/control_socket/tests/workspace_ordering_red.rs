//! Multiwindow routing regressions for workspace ordering.

use super::*;

const A1: &str = "11111111-1111-4111-8111-111111111111";
const A2: &str = "22222222-2222-4222-8222-222222222222";
const B1: &str = "33333333-3333-4333-8333-333333333333";
const B2: &str = "44444444-4444-4444-8444-444444444444";

fn ordering_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("window-a".into());
    snapshot.windows[0].tab_manager.workspaces[0].workspace_id = Some(A1.into());
    let mut a2 = snapshot.windows[0].tab_manager.workspaces[0].clone();
    a2.workspace_id = Some(A2.into());
    snapshot.windows[0].tab_manager.workspaces.push(a2);

    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("window-b".into());
    second.tab_manager.workspaces[0].workspace_id = Some(B1.into());
    second.tab_manager.workspaces[1].workspace_id = Some(B2.into());
    snapshot.windows.push(second);
    snapshot
}

#[test]
fn ordering_resolves_the_workspace_inside_its_routed_window() {
    let snapshot = ordering_snapshot();
    assert_eq!(workspace_index_for_id_in_window(&snapshot, 1, B2), Some(1));
    assert_eq!(workspace_index_for_id_in_window(&snapshot, 0, B2), None);
}

#[test]
fn batch_ordering_prefers_explicit_window_then_first_workspace_owner() {
    let snapshot = ordering_snapshot();
    let ids = [Uuid::parse_str(B2).unwrap()];

    let explicit = serde_json::Map::from_iter([("window_id".into(), json!("window-a"))]);
    assert_eq!(
        workspace_reorder_many_window_index_with_active(
            &snapshot,
            &explicit,
            &ids,
            Some("window-b")
        ),
        Some(0)
    );
    assert_eq!(
        workspace_reorder_many_window_index_with_active(
            &snapshot,
            &serde_json::Map::new(),
            &ids,
            Some("window-a")
        ),
        Some(1)
    );

    let invalid = serde_json::Map::from_iter([("window_id".into(), json!("missing"))]);
    assert_eq!(
        workspace_reorder_many_window_index_with_active(
            &snapshot,
            &invalid,
            &ids,
            Some("window-a")
        ),
        None
    );
}

#[test]
fn ordering_event_uses_the_exact_canonical_lifecycle_envelope() {
    let mut snapshot = ordering_snapshot();
    snapshot.windows[1].tab_manager.workspaces.swap(0, 1);
    let event = workspace_reordered_event_spec(&snapshot, 1, &[B2.to_string()])
        .expect("routed workspace ordering event");

    assert_eq!(event.name, "workspace.reordered");
    assert_eq!(event.category, "workspace");
    assert_eq!(event.source, "workspace.lifecycle");
    assert_eq!(event.window_id, None);
    assert_eq!(event.workspace_id.as_deref(), Some(B2));
    assert_eq!(event.surface_id, None);
    assert_eq!(
        event.payload,
        json!({
            "workspace_ids": [B2, B1],
            "moved_workspace_ids": [B2],
            "pinned_workspace_ids": [],
            "count": 2,
        })
    );
}

#[test]
fn move_event_wraps_the_original_request_and_stable_result() {
    let params = serde_json::Map::from_iter([
        ("workspace_id".into(), json!(B2)),
        ("window_id".into(), json!("window-c")),
        ("focus".into(), json!(false)),
    ]);
    let result = json!({
        "workspace_id": B2,
        "workspace_ref": "workspace:4",
        "window_id": "window-c",
        "window_ref": "window:3",
    });
    let event = workspace_moved_event_spec(&params, &result).expect("workspace move event");

    assert_eq!(event.name, "workspace.moved");
    assert_eq!(event.category, "workspace");
    assert_eq!(event.source, "socket.v2");
    assert_eq!(event.window_id.as_deref(), Some("window-c"));
    assert_eq!(event.workspace_id.as_deref(), Some(B2));
    assert_eq!(event.surface_id, None);
    assert_eq!(
        event.payload,
        json!({
            "method": "workspace.move_to_window",
            "params": params,
            "result": result,
        })
    );
}
