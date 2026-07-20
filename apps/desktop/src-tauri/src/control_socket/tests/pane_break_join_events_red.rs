use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::pane_surface_lifecycle_red::resizable_snapshot;
use super::*;
use cmux_core::session::{SessionPanePublishedSelectionSnapshot, SessionWorkspaceLayoutSnapshot};
use serde_json::{json, Value};

fn transition(method: &str, snapshot: &AppSessionSnapshot, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        method,
        params.as_object().expect("decoded params object"),
        &LifecycleDispatchContext::new(true, true, None),
    )
}

fn ok_value(transition: &LifecycleTransition) -> Value {
    let ControlCallResult::Ok(value) = &transition.result else {
        panic!(
            "expected successful lifecycle transition: {:?}",
            transition.result
        )
    };
    value.clone().into()
}

fn pane<'a>(
    layout: Option<&'a SessionWorkspaceLayoutSnapshot>,
    pane_id: &str,
) -> Option<&'a cmux_core::session::SessionPaneLayoutSnapshot> {
    match layout? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            (pane.pane_id.as_deref() == Some(pane_id)).then_some(pane)
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            pane(Some(&split.first), pane_id).or_else(|| pane(Some(&split.second), pane_id))
        }
    }
}

fn seed_published_selection(snapshot: &mut AppSessionSnapshot, pane_id: &str, surface_id: &str) {
    snapshot.windows[0].tab_manager.workspaces[0].published_pane_selections =
        Some(vec![SessionPanePublishedSelectionSnapshot {
            pane_id: pane_id.into(),
            panel_id: surface_id.into(),
        }]);
}

#[test]
fn pane_break_and_join_use_the_explicit_lifecycle_route() {
    for method in ["pane.break", "pane.join"] {
        assert_eq!(
            control_request_route_for_method(method),
            ControlRequestRoute::PaneSurfaceLifecycle,
            "{method} must suppress legacy session.model derivation"
        );
    }
}

#[test]
fn pane_break_publishes_fallback_detach_attach_and_completion_in_canonical_order() {
    let mut snapshot = resizable_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-right".into());

    let broken = transition(
        "pane.break",
        &snapshot,
        json!({
            "workspace_id": "workspace-1",
            "pane_id": "pane-right",
            "focus": false,
        }),
    );
    let result = ok_value(&broken);
    let destination_workspace_id = result["workspace_id"].as_str().expect("workspace id");
    let destination_pane_id = result["pane_id"].as_str().expect("pane id");
    assert_eq!(result["window_id"], json!("window-1"));
    assert_eq!(result["surface_id"], json!("surface-right"));
    assert_ne!(destination_workspace_id, "workspace-1");
    assert_ne!(destination_pane_id, "pane-right");

    assert_eq!(
        broken
            .events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        [
            "surface.selected",
            "pane.focused",
            "surface.focused",
            "surface.closed",
            "surface.created",
            "pane.broken",
        ]
    );
    assert!(broken.events[..5]
        .iter()
        .all(|event| event.source == "workspace.lifecycle" && event.window_id.is_none()));
    assert!(broken
        .events
        .iter()
        .all(|event| event.source != "session.model"));
    assert_eq!(
        broken.events[0].payload,
        json!({
            "focused": true,
            "kind": "terminal",
            "origin": "bonsplit_selection",
            "pane_id": "pane-left",
            "previous_surface_id": null,
            "surface_id": "surface-left",
        })
    );
    assert_eq!(
        broken.events[3].workspace_id.as_deref(),
        Some("workspace-1")
    );
    assert_eq!(broken.events[3].pane_id.as_deref(), Some("pane-right"));
    assert_eq!(
        broken.events[3].payload,
        json!({
            "kind": "terminal",
            "origin": "detach",
            "pane_id": "pane-right",
            "surface_id": "surface-right",
        })
    );
    assert_eq!(
        broken.events[4].payload,
        json!({
            "focused": false,
            "kind": "terminal",
            "origin": "detach_attach",
            "pane_id": destination_pane_id,
            "surface_id": "surface-right",
        })
    );
    assert_eq!(
        broken.events[4].workspace_id.as_deref(),
        Some(destination_workspace_id)
    );
    assert_eq!(
        broken.events[5].payload,
        json!({
            "method": "pane.break",
            "params": {
                "focus": false,
                "pane_id": "pane-right",
                "workspace_id": "workspace-1",
            },
            "result": result,
        })
    );
    assert_eq!(broken.effects, [LifecycleEffect::PersistSession]);

    let source = broken.snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id.as_deref() == Some("workspace-1"))
        .expect("source workspace");
    assert_eq!(source.focused_panel_id.as_deref(), Some("surface-left"));
    assert!(pane(source.layout.as_ref(), "pane-right").is_none());
    let destination = broken.snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id.as_deref() == Some(destination_workspace_id))
        .expect("destination workspace");
    assert_eq!(
        destination.focused_panel_id.as_deref(),
        Some("surface-right")
    );
    assert_eq!(
        pane(destination.layout.as_ref(), destination_pane_id)
            .expect("destination pane")
            .selected_panel_id
            .as_deref(),
        Some("surface-right")
    );
}

#[test]
fn pane_join_selects_the_moved_surface_and_publishes_canonical_focus_sequence() {
    let mut snapshot = resizable_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-right".into());
    seed_published_selection(&mut snapshot, "pane-left", "surface-left");

    let joined = transition(
        "pane.join",
        &snapshot,
        json!({
            "pane_id": "pane-right",
            "target_pane_id": "pane-left",
            "focus": false,
        }),
    );
    let result = ok_value(&joined);
    assert_eq!(
        result,
        json!({
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "pane_id": "pane-left",
            "surface_id": "surface-right",
        })
    );
    assert_eq!(
        joined
            .events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        [
            "surface.selected",
            "pane.focused",
            "surface.focused",
            "pane.joined",
        ]
    );
    assert!(joined.events[..3]
        .iter()
        .all(|event| event.source == "workspace.lifecycle" && event.window_id.is_none()));
    assert!(joined
        .events
        .iter()
        .all(|event| event.source != "session.model"));
    assert_eq!(
        joined.events[0].payload,
        json!({
            "focused": true,
            "kind": "terminal",
            "origin": "bonsplit_selection",
            "pane_id": "pane-left",
            "previous_surface_id": "surface-left",
            "surface_id": "surface-right",
        })
    );
    assert_eq!(
        joined.events[3].payload,
        json!({
            "method": "pane.join",
            "params": {
                "focus": false,
                "pane_id": "pane-right",
                "target_pane_id": "pane-left",
            },
            "result": result,
        })
    );
    assert_eq!(joined.effects, [LifecycleEffect::PersistSession]);

    let workspace = &joined.snapshot.windows[0].tab_manager.workspaces[0];
    assert_eq!(workspace.focused_panel_id.as_deref(), Some("surface-right"));
    assert!(pane(workspace.layout.as_ref(), "pane-right").is_none());
    let target = pane(workspace.layout.as_ref(), "pane-left").expect("target pane");
    assert_eq!(target.panel_ids, ["surface-left", "surface-right"]);
    assert_eq!(target.selected_panel_id.as_deref(), Some("surface-right"));
}

#[test]
fn pane_join_omits_redundant_pane_focus_when_target_pane_was_already_focused() {
    let mut snapshot = resizable_snapshot();
    seed_published_selection(&mut snapshot, "pane-left", "surface-left");

    let joined = transition(
        "pane.join",
        &snapshot,
        json!({
            "pane_id": "pane-right",
            "target_pane_id": "pane-left",
        }),
    );

    assert_eq!(
        joined
            .events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        ["surface.selected", "surface.focused", "pane.joined"]
    );
}

#[test]
fn pane_break_and_join_resolve_public_refs_and_explicit_surfaces_on_the_pure_path() {
    let snapshot = resizable_snapshot();
    let broken = transition(
        "pane.break",
        &snapshot,
        json!({
            "workspace_id": "workspace-1",
            "pane_ref": "pane:2",
        }),
    );
    assert_eq!(ok_value(&broken)["surface_id"], json!("surface-right"));

    let joined = transition(
        "pane.join",
        &snapshot,
        json!({
            "workspace_id": "workspace-1",
            "surface_id": "surface-right",
            "target_pane_ref": "pane:1",
        }),
    );
    assert_eq!(
        ok_value(&joined),
        json!({
            "window_id": "window-1",
            "workspace_id": "workspace-1",
            "pane_id": "pane-left",
            "surface_id": "surface-right",
        })
    );
}
