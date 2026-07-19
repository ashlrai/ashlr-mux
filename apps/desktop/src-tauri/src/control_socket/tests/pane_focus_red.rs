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
