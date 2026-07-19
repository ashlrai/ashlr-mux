use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::pane_surface_lifecycle_red::resizable_snapshot;
use super::*;
use serde_json::{json, Value};

fn transition(params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        &resizable_snapshot(),
        "pane.swap",
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
fn pane_swap_uses_the_explicit_lifecycle_route() {
    assert_eq!(
        control_request_route_for_method("pane.swap"),
        ControlRequestRoute::PaneSurfaceLifecycle
    );
}

#[test]
fn pane_swap_publishes_the_canonical_placeholder_move_sequence() {
    let swapped = transition(json!({
        "pane_id": "pane-left",
        "target_pane_id": "pane-right",
        "focus": false,
    }));
    let result = json!({
        "window_id": "window-1",
        "workspace_id": "workspace-1",
        "pane_id": "pane-left",
        "target_pane_id": "pane-right",
        "source_surface_id": "surface-left",
        "target_surface_id": "surface-right",
    });
    assert_eq!(ok_value(&swapped), result);

    let placeholder = swapped.events[0]
        .surface_id
        .as_deref()
        .expect("placeholder surface id");
    assert!(uuid::Uuid::parse_str(placeholder).is_ok());
    assert_ne!(placeholder, "surface-left");
    assert_ne!(placeholder, "surface-right");
    assert_eq!(
        serde_json::to_value(&swapped.events).expect("serialize pane.swap events"),
        json!([
            {
                "name": "surface.created",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": placeholder,
                "payload": {
                    "focused": false,
                    "kind": "terminal",
                    "origin": "terminal_tab",
                    "pane_id": "pane-right",
                    "surface_id": placeholder,
                },
            },
            {
                "name": "surface.selected",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": "surface-left",
                "payload": {
                    "focused": true,
                    "kind": "terminal",
                    "origin": "bonsplit_selection",
                    "pane_id": "pane-right",
                    "previous_surface_id": "surface-right",
                    "surface_id": "surface-left",
                },
            },
            {
                "name": "pane.focused",
                "category": "pane",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": "surface-left",
                "payload": {
                    "origin": "bonsplit_selection",
                    "pane_id": "pane-right",
                    "selected_surface_id": "surface-left",
                },
            },
            {
                "name": "surface.selected",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-left",
                "surface_id": "surface-right",
                "payload": {
                    "focused": true,
                    "kind": "terminal",
                    "origin": "bonsplit_selection",
                    "pane_id": "pane-left",
                    "previous_surface_id": "surface-left",
                    "surface_id": "surface-right",
                },
            },
            {
                "name": "pane.focused",
                "category": "pane",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-left",
                "surface_id": "surface-right",
                "payload": {
                    "origin": "bonsplit_selection",
                    "pane_id": "pane-left",
                    "selected_surface_id": "surface-right",
                },
            },
            {
                "name": "surface.focused",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-left",
                "surface_id": "surface-right",
                "payload": {
                    "kind": "terminal",
                    "origin": "bonsplit_selection",
                    "pane_id": "pane-left",
                    "surface_id": "surface-right",
                },
            },
            {
                "name": "surface.closed",
                "category": "surface",
                "source": "workspace.lifecycle",
                "window_id": null,
                "workspace_id": "workspace-1",
                "pane_id": "pane-right",
                "surface_id": placeholder,
                "payload": {
                    "kind": "terminal",
                    "origin": "tab_close",
                    "pane_id": "pane-right",
                    "surface_id": placeholder,
                },
            },
            {
                "name": "pane.swapped",
                "category": "pane",
                "source": "socket.v2",
                "window_id": "window-1",
                "workspace_id": "workspace-1",
                "pane_id": "pane-left",
                "surface_id": null,
                "payload": {
                    "method": "pane.swap",
                    "params": {
                        "focus": false,
                        "pane_id": "pane-left",
                        "target_pane_id": "pane-right",
                    },
                    "result": result,
                },
            },
        ])
    );

    let workspace = &swapped.snapshot.windows[0].tab_manager.workspaces[0];
    assert_eq!(
        pane_snapshot_at_index(workspace, 0)
            .expect("source pane")
            .selected_panel_id
            .as_deref(),
        Some("surface-right")
    );
    assert_eq!(
        pane_snapshot_at_index(workspace, 1)
            .expect("target pane")
            .selected_panel_id
            .as_deref(),
        Some("surface-left")
    );
    assert_eq!(workspace.focused_panel_id.as_deref(), Some("surface-left"));
    assert_eq!(swapped.effects, [LifecycleEffect::PersistSession]);
}

#[test]
fn pane_swap_accepts_indexed_refs_and_normalizes_completion_params() {
    let swapped = transition(json!({
        "pane_ref": "pane:1",
        "target_pane_ref": "pane:2",
    }));

    assert_eq!(ok_value(&swapped)["pane_id"], json!("pane-left"));
    assert_eq!(
        swapped.events.last().expect("completion").payload["params"],
        json!({
            "focus": false,
            "pane_id": "pane-left",
            "target_pane_id": "pane-right",
        })
    );
}

#[test]
fn pane_swap_rejects_the_same_pane_without_mutation_or_events() {
    let rejected = transition(json!({
        "pane_id": "pane-left",
        "target_pane_id": "pane-left",
    }));

    assert_eq!(rejected.snapshot, resizable_snapshot());
    assert!(rejected.events.is_empty());
    assert!(rejected.effects.is_empty());
    assert_eq!(
        rejected.result,
        ControlCallResult::Err {
            code: "invalid_params".into(),
            message: "pane_id and target_pane_id must be different".into(),
            data: None,
        }
    );
}
