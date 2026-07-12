//! Adversarial Dock contracts beyond the primary eight-case lifecycle matrix.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::*;
use crate::dock::{DockCreateRequest, DockStore, DockSurfaceKind};
use cmux_core::surface_lifecycle::SurfaceLifecycleModel;
use std::collections::BTreeSet;

const W1: &str = "10000000-0000-0000-0000-000000000001";
const W2: &str = "10000000-0000-0000-0000-000000000002";

fn context() -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1_000.0, 800.0)),
        browser_enabled: true,
        dock_available: true,
        active_window_id: Some(W1.into()),
    }
}

fn windows() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some(W1.into());
    let mut second = snapshot.windows[0].clone();
    second.window_id = Some(W2.into());
    second.selected_workspace_id = None;
    second.tab_manager.workspaces[0].workspace_id = Some("workspace-two".into());
    second.tab_manager.workspaces[0].focused_panel_id = Some("surface-two".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        second.tab_manager.workspaces[0].layout.as_mut().unwrap()
    else {
        unreachable!()
    };
    pane.pane_id = Some("pane-two".into());
    pane.panel_ids = vec!["surface-two".into()];
    pane.selected_panel_id = Some("surface-two".into());
    snapshot.windows.push(second);
    snapshot
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(snapshot, method, params.as_object().unwrap(), &context())
}

fn ok(transition: &LifecycleTransition) -> Value {
    let ControlCallResult::Ok(value) = &transition.result else {
        panic!("expected success, got {:?}", transition.result)
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
        panic!("expected error, got {:?}", transition.result)
    };
    assert_eq!(
        (actual_code.as_str(), actual_message.as_str()),
        (code, message)
    );
    data.clone().map(Value::from).unwrap_or(Value::Null)
}

fn effect(transition: &LifecycleTransition, tag: &str) -> Value {
    let encoded = serde_json::to_value(&transition.effects).unwrap();
    encoded
        .as_array()
        .unwrap()
        .iter()
        .find_map(|effect| effect.get(tag).cloned())
        .unwrap_or_else(|| panic!("missing {tag} effect: {encoded}"))
}

fn seed_dock(
    snapshot: &mut AppSessionSnapshot,
    owner: &str,
    kind: DockSurfaceKind,
) -> crate::dock::DockCreateResult {
    DockStore
        .create(
            snapshot,
            owner,
            DockCreateRequest {
                kind,
                focus: true,
                url: (kind == DockSurfaceKind::Browser).then(|| "https://seed.test".to_string()),
                ..DockCreateRequest::default()
            },
        )
        .unwrap()
}

#[test]
fn cross_dock_move_publishes_both_source_and_destination_owners_once() {
    let mut snapshot = windows();
    let source = seed_dock(&mut snapshot, W1, DockSurfaceKind::Terminal);
    let destination = seed_dock(&mut snapshot, W2, DockSurfaceKind::Terminal);

    let moved = transition(
        &snapshot,
        "surface.move",
        json!({
            "surface_id":source.surface_id,
            "pane_id":destination.pane_id,
            "index":0,
            "focus":false,
        }),
    );
    assert_eq!(ok(&moved)["window_id"], W2);
    let owners = moved
        .effects
        .iter()
        .filter_map(|effect| match effect {
            LifecycleEffect::DockChanged { owner_id, .. } => Some(owner_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(owners, vec![W1, W2]);
    assert_eq!(owners.iter().copied().collect::<BTreeSet<_>>().len(), 2);

    let model = SurfaceLifecycleModel::from_app_session(&moved.snapshot).unwrap();
    assert_eq!(
        model
            .owner_of_surface(&source.surface_id.to_string())
            .unwrap()
            .window_id,
        W2
    );
}

#[test]
fn malformed_uuid_selectors_fall_back_like_frozen_routing_before_dock_creation() {
    let mut snapshot = windows();
    let seeded = seed_dock(&mut snapshot, W1, DockSurfaceKind::Terminal);
    for params in [
        json!({"placement":"dock", "workspace_id":"not-a-uuid", "type":"terminal"}),
        json!({"placement":"dock", "surface_id":"not-a-uuid", "type":"terminal"}),
        json!({"placement":"dock", "pane_id":"not-a-uuid", "type":"terminal"}),
    ] {
        let created = transition(&snapshot, "surface.create", params);
        let value = ok(&created);
        assert_eq!(value["window_id"], W1);
        assert_eq!(value["workspace_id"], W1);
        assert!(value["dock_surface_id"].is_string());
        snapshot = created.snapshot;
    }
    assert!(DockStore
        .list(&snapshot, W1)
        .iter()
        .any(|surface| surface.surface_id == seeded.surface_id));

    for params in [
        json!({"placement":"dock", "workspace_id":Uuid::new_v4(), "type":"terminal"}),
        json!({"placement":"dock", "surface_id":Uuid::new_v4(), "type":"terminal"}),
    ] {
        let created = transition(&snapshot, "surface.create", params);
        assert_eq!(ok(&created)["window_id"], W1);
        snapshot = created.snapshot;
    }

    let missing_pane = transition(
        &snapshot,
        "surface.create",
        json!({"placement":"dock", "pane_id":Uuid::new_v4(), "type":"terminal"}),
    );
    assert_error(&missing_pane, "not_found", "Pane not found");

    let unsupported = transition(
        &windows(),
        "surface.create",
        json!({"placement":"dock", "workspace_id":"not-a-uuid", "type":"markdown"}),
    );
    assert_error(
        &unsupported,
        "invalid_params",
        "Dock placement supports only terminal and browser surfaces",
    );
}

#[test]
fn dock_focus_uses_shared_v2_bool_values_and_reveals_only_the_owner() {
    for focus in [json!(1), json!("true"), json!("yes"), json!("on")] {
        let created = transition(
            &windows(),
            "surface.create",
            json!({
                "placement":"dock",
                "window_id":W2,
                "type":"terminal",
                "focus":focus,
            }),
        );
        let value = ok(&created);
        let surface_id = value["dock_surface_id"].as_str().unwrap();
        let model = SurfaceLifecycleModel::from_app_session(&created.snapshot).unwrap();
        assert_eq!(
            model.focused_surface(&format!("dock:{W2}")),
            Some(surface_id)
        );
        let reveal_owners = created
            .effects
            .iter()
            .filter_map(|effect| match effect {
                LifecycleEffect::DockReveal { owner_id } => Some(owner_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reveal_owners, vec![W2]);
    }
}

#[test]
fn dock_browser_runtime_effect_is_owner_aware() {
    let created = transition(
        &windows(),
        "surface.create",
        json!({
            "placement":"dock",
            "window_id":W2,
            "type":"browser",
            "url":"https://profile.test",
            "focus":false,
        }),
    );
    let value = ok(&created);
    assert_eq!(value["window_id"], W2);
    let dock_create = effect(&created, "DockCreate");
    assert_eq!(dock_create["owner_id"], W2);
    assert_eq!(dock_create["intent"]["type"], "browser");
    assert_eq!(dock_create["intent"]["url"], "https://profile.test");
}

#[test]
fn dock_effects_encode_canonical_failure_mapping_and_visibility_boundaries() {
    for (method, mut params, expected) in [
        (
            "surface.create",
            json!({"placement":"dock", "type":"browser", "url":"https://stage.test"}),
            "Failed to create surface",
        ),
        (
            "pane.create",
            json!({"placement":"dock", "type":"browser", "url":"https://stage.test", "direction":"right"}),
            "Failed to create pane",
        ),
    ] {
        params["window_id"] = json!(W1);
        let created = transition(&windows(), method, params);
        let runtime = effect(&created, "DockCreate");
        assert_eq!(runtime["failure_code"], "internal_error");
        assert_eq!(runtime["failure_message"], expected);
        assert_eq!(runtime["visibility_phase"], "post_persist");
        assert_eq!(runtime["rollback"], "teardown");

        let changed = effect(&created, "DockChanged");
        assert_eq!(changed["phase"], "post_persist");
    }
}

#[test]
fn dock_close_requires_runtime_teardown_before_publishing_changed_state() {
    let mut snapshot = windows();
    let created = seed_dock(&mut snapshot, W1, DockSurfaceKind::Terminal);
    let closed = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id":created.surface_id}),
    );
    ok(&closed);
    let teardown = effect(&closed, "RuntimeTeardown");
    assert_eq!(teardown["must_succeed"], true);
    assert_eq!(teardown["failure_code"], "internal_error");
    assert_eq!(teardown["failure_message"], "Failed to close surface");
    assert_eq!(teardown["phase"], "pre_publish");
    assert_eq!(effect(&closed, "DockChanged")["phase"], "post_persist");
}
