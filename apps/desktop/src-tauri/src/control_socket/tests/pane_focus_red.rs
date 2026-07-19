use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleTransition,
};
use super::pane_surface_lifecycle_red::{resizable_snapshot, two_window_snapshot};
use super::*;
use serde_json::{json, Value};

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        method,
        params.as_object().expect("decoded params object"),
        &LifecycleDispatchContext {
            browser_enabled: true,
            dock_available: true,
            active_window_id: None,
        },
    )
}

fn ok_value(transition: &LifecycleTransition) -> Value {
    let ControlCallResult::Ok(value) = &transition.result else {
        panic!("expected successful lifecycle transition")
    };
    value.clone().into()
}

fn assert_error(transition: &LifecycleTransition, code: &str, message: &str) -> Value {
    let ControlCallResult::Err {
        code: actual_code,
        message: actual_message,
        data,
    } = &transition.result
    else {
        panic!("expected lifecycle transition error")
    };
    assert_eq!(actual_code, code);
    assert_eq!(actual_message, message);
    data.clone().map(Value::from).unwrap_or(Value::Null)
}

#[test]
fn pane_focus_returns_only_the_canonical_owner_identity() {
    let focused = transition(
        &resizable_snapshot(),
        "pane.focus",
        json!({"workspace_id": "workspace-1", "pane_id": "pane-right"}),
    );

    assert_eq!(
        ok_value(&focused),
        json!({
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "pane_id": "pane-right",
        })
    );
}

#[test]
fn pane_focus_rejects_a_pane_outside_the_resolved_scope() {
    let snapshot = two_window_snapshot();
    let focused = transition(
        &snapshot,
        "pane.focus",
        json!({
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "pane_id": "pane-2",
        }),
    );

    assert_eq!(
        assert_error(&focused, "not_found", "Pane not found"),
        json!({"pane_id": "pane-2"})
    );
    assert_eq!(focused.snapshot, snapshot);
    assert!(!focused.changed);
}

fn two_workspace_focus_snapshot() -> AppSessionSnapshot {
    let mut snapshot = resizable_snapshot();
    let window = &mut snapshot.windows[0];
    window.selected_workspace_id = Some("workspace-1".into());
    window.tab_manager.selected_workspace_index = Some(0);

    let mut target = crate::session::fresh_control_window_workspace("surface-target");
    target.workspace_id = Some("workspace-target".into());
    target.process_title = "target-shell".into();
    target.custom_title = Some("Target workspace".into());
    target.current_directory = Some("C:/target".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        target.layout.as_mut().expect("target workspace layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-target".into());
    for surface in target.surfaces.as_mut().into_iter().flatten() {
        surface.pane_id = "pane-target".into();
    }
    window.tab_manager.workspaces.push(target);
    snapshot
}

#[test]
fn pane_focus_publishes_the_canonical_cross_workspace_lifecycle_sequence() {
    let focused = transition(
        &two_workspace_focus_snapshot(),
        "pane.focus",
        json!({"workspace_id": "workspace-target", "pane_id": "pane-target"}),
    );

    assert_eq!(
        serde_json::to_value(&focused.events).expect("serialize focus events"),
        json!([
            {
                "name": "window.focused",
                "category": "window",
                "source": "window.lifecycle",
                "window_id": "window-1",
                "workspace_id": "workspace-1",
                "pane_id": null,
                "surface_id": null,
                "payload": {
                    "window_id": "window-1",
                    "workspace_id": "workspace-1",
                    "workspace_count": 2,
                    "selected_workspace_index": 0,
                    "is_key_window": true,
                    "is_main_window": true,
                    "origin": "focus_request",
                },
            },
            {
                "name": "workspace.selected",
                "category": "workspace",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-target",
                "pane_id": null,
                "surface_id": null,
                "payload": {
                    "workspace_id": "workspace-target",
                    "title": "Target workspace",
                    "custom_title": "Target workspace",
                    "cwd": "C:/target",
                    "index": 1,
                    "selected": true,
                    "tab_count": 2,
                    "previous_workspace_id": "workspace-1",
                },
            },
            {
                "name": "surface.selected",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-target",
                "pane_id": "pane-target",
                "surface_id": "surface-target",
                "payload": {
                    "surface_id": "surface-target",
                    "pane_id": "pane-target",
                    "kind": "terminal",
                    "focused": true,
                    "previous_surface_id": null,
                    "origin": "bonsplit_selection",
                },
            },
            {
                "name": "pane.focused",
                "category": "pane",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-target",
                "pane_id": "pane-target",
                "surface_id": "surface-target",
                "payload": {
                    "pane_id": "pane-target",
                    "selected_surface_id": "surface-target",
                    "origin": "bonsplit_selection",
                },
            },
            {
                "name": "surface.focused",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-target",
                "pane_id": "pane-target",
                "surface_id": "surface-target",
                "payload": {
                    "surface_id": "surface-target",
                    "pane_id": "pane-target",
                    "kind": "terminal",
                    "origin": "bonsplit_selection",
                },
            },
        ])
    );
}

#[test]
fn pane_focus_omits_workspace_selection_when_the_workspace_is_already_selected() {
    let mut snapshot = resizable_snapshot();
    snapshot.windows[0].selected_workspace_id = Some("workspace-1".into());
    let focused = transition(
        &snapshot,
        "pane.focus",
        json!({"workspace_id": "workspace-1", "pane_id": "pane-right"}),
    );

    assert_eq!(
        focused
            .events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        [
            "window.focused",
            "surface.selected",
            "pane.focused",
            "surface.focused",
        ]
    );
}
