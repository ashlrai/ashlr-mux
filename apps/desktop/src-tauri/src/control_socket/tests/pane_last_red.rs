use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleTransition,
};
use super::pane_surface_lifecycle_red::resizable_snapshot;
use super::*;
use serde_json::{json, Value};

fn transition(snapshot: &AppSessionSnapshot, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        "pane.last",
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

#[test]
fn pane_last_uses_the_explicit_lifecycle_route() {
    assert_eq!(
        control_request_route_for_method("pane.last"),
        ControlRequestRoute::PaneSurfaceLifecycle
    );
}

#[test]
fn pane_last_publishes_only_the_canonical_selection_sequence() {
    let last = transition(
        &resizable_snapshot(),
        json!({"workspace_id": "workspace-1"}),
    );

    assert_eq!(
        ok_value(&last),
        json!({
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "pane_id": "pane-right",
            "surface_id": "surface-right",
        })
    );
    assert_eq!(
        serde_json::to_value(&last.events).expect("serialize pane.last events"),
        json!([
            {
                "name": "surface.selected",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": "surface-right",
                "payload": {
                    "surface_id": "surface-right",
                    "pane_id": "pane-right",
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
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": "surface-right",
                "payload": {
                    "pane_id": "pane-right",
                    "selected_surface_id": "surface-right",
                    "origin": "bonsplit_selection",
                },
            },
            {
                "name": "surface.focused",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": "surface-right",
                "payload": {
                    "surface_id": "surface-right",
                    "pane_id": "pane-right",
                    "kind": "terminal",
                    "origin": "bonsplit_selection",
                },
            },
        ])
    );
    assert_eq!(
        last.snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-right")
    );
}

#[test]
fn cli_last_pane_omits_the_direct_api_selection_callback() {
    let last = transition(
        &resizable_snapshot(),
        json!({
            "__cmux_cli_command": "last-pane",
            "workspace_id": "workspace-1",
        }),
    );

    assert_eq!(
        last.events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        ["pane.focused", "surface.focused"]
    );
}
