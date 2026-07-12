//! Adversarial frozen-e1825d40 contracts not covered by the happy-path action matrix.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::*;
use cmux_core::session::{
    SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot, SessionSurfaceSnapshot,
};
use cmux_core::surface_lifecycle::SurfaceLifecycleModel;

const W1: &str = "10000000-0000-0000-0000-000000000001";
const W2: &str = "10000000-0000-0000-0000-000000000002";
const WS1: &str = "20000000-0000-0000-0000-000000000001";
const WS2: &str = "20000000-0000-0000-0000-000000000002";
const P1: &str = "30000000-0000-0000-0000-000000000001";
const P2: &str = "30000000-0000-0000-0000-000000000002";
const A: &str = "40000000-0000-0000-0000-000000000001";
const B: &str = "40000000-0000-0000-0000-000000000002";
const C: &str = "40000000-0000-0000-0000-000000000003";
const OTHER: &str = "40000000-0000-0000-0000-000000000004";

fn context(browser_enabled: bool) -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1200.0, 800.0)),
        browser_enabled,
        dock_available: true,
        active_window_id: Some(W1.into()),
    }
}

fn surface(id: &str, pane_id: &str, kind: SessionSurfaceKindSnapshot) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: id.into(),
        pane_id: pane_id.into(),
        generation: 1,
        kind,
        metadata: SessionSurfaceMetadataSnapshot::default(),
        terminal_startup: None,
    }
}

fn browser(url: &str) -> SessionSurfaceKindSnapshot {
    SessionSurfaceKindSnapshot::Browser {
        url: Some(url.into()),
        profile: None,
        proxy_url: None,
        back_history: None,
        forward_history: None,
        omnibar_visible: None,
        focus_mode_active: None,
        developer_tools_visible: None,
        developer_tools_panel: None,
        page_zoom: None,
    }
}

fn action_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let window = &mut snapshot.windows[0];
    window.window_id = Some(W1.into());
    window.selected_workspace_id = Some(WS1.into());
    let workspace = &mut window.tab_manager.workspaces[0];
    workspace.workspace_id = Some(WS1.into());
    workspace.focused_panel_id = Some(A.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.pane_id = Some(P1.into());
    pane.panel_ids = vec![A.into(), B.into(), C.into()];
    pane.selected_panel_id = Some(A.into());
    workspace.surfaces = Some(vec![
        surface(A, P1, SessionSurfaceKindSnapshot::Terminal),
        surface(B, P1, browser("https://example.test")),
        surface(C, P1, SessionSurfaceKindSnapshot::Terminal),
    ]);
    snapshot
}

fn two_window_snapshot() -> AppSessionSnapshot {
    let mut snapshot = action_snapshot();
    let mut second = snapshot.windows[0].clone();
    second.window_id = Some(W2.into());
    second.selected_workspace_id = Some(WS2.into());
    let workspace = &mut second.tab_manager.workspaces[0];
    workspace.workspace_id = Some(WS2.into());
    workspace.focused_panel_id = Some(OTHER.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.pane_id = Some(P2.into());
    pane.panel_ids = vec![OTHER.into()];
    pane.selected_panel_id = Some(OTHER.into());
    workspace.surfaces = Some(vec![surface(
        OTHER,
        P2,
        SessionSurfaceKindSnapshot::Terminal,
    )]);
    snapshot.windows.push(second);
    snapshot
}

fn dispatch_with(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: Value,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    dispatch_lifecycle_request(snapshot, method, params.as_object().unwrap(), context)
}

fn dispatch(snapshot: &AppSessionSnapshot, params: Value) -> LifecycleTransition {
    dispatch_with(snapshot, "surface.action", params, &context(true))
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

fn error_pair(transition: &LifecycleTransition) -> (&str, &str) {
    let ControlCallResult::Err { code, message, .. } = &transition.result else {
        panic!("expected error, got {:?}", transition.result)
    };
    (code.as_str(), message.as_str())
}

fn model(snapshot: &AppSessionSnapshot) -> SurfaceLifecycleModel {
    SurfaceLifecycleModel::from_app_session_snapshot(snapshot).unwrap()
}

fn serialized_effect(transition: &LifecycleTransition, tag: &str) -> Value {
    let encoded = serde_json::to_value(&transition.effects).unwrap();
    encoded
        .as_array()
        .unwrap()
        .iter()
        .find(|effect| effect.get(tag).is_some())
        .unwrap_or_else(|| panic!("missing {tag} effect: {encoded}"))[tag]
        .clone()
}

#[test]
fn remote_terminal_action_requests_new_window_and_defers_identity_to_runtime_arrival() {
    let mut snapshot = action_snapshot();
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["remote"] = json!({
        "enabled": true,
        "connected": true,
        "state": "connected",
        "transport": "tmux",
        "destination": "host.example",
        "persistent_daemon_slot": "remote-session-1"
    });
    snapshot = serde_json::from_value(encoded).unwrap();

    let transition = dispatch(
        &snapshot,
        json!({"surface_id": A, "action": "new-terminal-right"}),
    );
    let value = ok(&transition);
    assert_eq!(value["accepted"], true);
    assert!(value["created_surface_id"].is_null());
    assert_eq!(transition.snapshot, snapshot);
    let remote = serialized_effect(&transition, "RemoteCreate");
    assert_eq!(remote["tmux_operation"], "new-window");
    assert_eq!(remote["arrival_policy"], "runtime-window-add");
    assert_eq!(remote["failure_code"], "internal_error");
    assert_eq!(remote["failure_message"], "Failed to create tab");
    assert!(remote.get("fabricated_surface_id").is_none());
    assert!(remote.get("fabricated_pane_id").is_none());
}

#[test]
fn explicit_surface_must_belong_to_the_resolved_workspace_and_window() {
    let snapshot = two_window_snapshot();
    for params in [
        json!({"window_id": W1, "workspace_id": WS1, "surface_id": OTHER, "action": "pin"}),
        json!({"window_id": W2, "workspace_id": WS2, "surface_id": A, "action": "pin"}),
    ] {
        let transition = dispatch(&snapshot, params);
        let data = assert_error(&transition, "not_found", "Tab not found");
        assert!(data["surface_id"].is_string());
        assert_eq!(transition.snapshot, snapshot);
    }
}

#[test]
fn null_or_invalid_surface_selector_falls_through_to_tab_then_focus() {
    let snapshot = action_snapshot();
    for params in [
        json!({"surface_id": null, "tab_id": B, "action": "pin"}),
        json!({"surface_id": "not-a-uuid", "tab_id": B, "action": "pin"}),
    ] {
        let transition = dispatch(&snapshot, params);
        assert_eq!(ok(&transition)["surface_id"], B);
    }
    let focused = dispatch(
        &snapshot,
        json!({"surface_id": "not-a-uuid", "tab_id": "also-invalid", "action": "pin"}),
    );
    assert_eq!(ok(&focused)["surface_id"], A);
}

#[test]
fn missing_action_precedes_workspace_and_target_resolution_after_manager_resolution() {
    let mut no_windows = action_snapshot();
    no_windows.windows.clear();
    assert_error(
        &dispatch(&no_windows, json!({})),
        "unavailable",
        "TabManager not available",
    );

    let snapshot = action_snapshot();
    let missing_workspace = dispatch(&snapshot, json!({"workspace_id": WS2, "surface_id": A}));
    let mut no_focus = snapshot;
    no_focus.windows[0].tab_manager.workspaces[0].focused_panel_id = None;
    let missing_focus = dispatch(&no_focus, json!({}));
    assert_eq!(
        vec![error_pair(&missing_workspace), error_pair(&missing_focus)],
        vec![
            ("invalid_params", "Missing action"),
            ("invalid_params", "Missing action")
        ]
    );
}

fn remote_action_snapshot() -> AppSessionSnapshot {
    let mut snapshot = action_snapshot();
    let source = snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row.surface_id == A)
        .unwrap();
    source.metadata.reported_directory = Some("/srv/repo with spaces".into());
    source.metadata.directory_provenance = Some("remote_report".into());
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["remote"] = json!({
        "enabled": true,
        "connected": true,
        "state": "connected",
        "transport": "tmux",
        "destination": "host.example",
        "persistent_daemon_slot": "remote-session-1"
    });
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn remote_new_window_plan_preserves_focus_placement_and_confirmed_source_cwd() {
    let snapshot = remote_action_snapshot();
    let mut actual = Vec::new();
    for focus in [true, false] {
        let transition = dispatch(
            &snapshot,
            json!({"surface_id": A, "action": "new-terminal-right", "focus": focus}),
        );
        let remote = serialized_effect(&transition, "RemoteCreate");
        actual.push(json!({
            "focus": remote["focus"],
            "focus_mode": remote["focus_mode"],
            "placement": remote["placement"],
            "source_surface_id": remote["source_surface_id"],
            "working_directory": remote["working_directory"],
            "working_directory_source_surface_id": remote["working_directory_source_surface_id"]
        }));
        assert_eq!(remote["source_surface_id"], A);
    }
    assert_eq!(
        actual,
        vec![
            json!({
                "focus": true,
                "focus_mode": "focused",
                "placement": "after-source-window",
                "source_surface_id": A,
                "working_directory": "/srv/repo with spaces",
                "working_directory_source_surface_id": A
            }),
            json!({
                "focus": false,
                "focus_mode": "background",
                "placement": "after-source-window",
                "source_surface_id": A,
                "working_directory": "/srv/repo with spaces",
                "working_directory_source_surface_id": A
            })
        ]
    );
}

#[test]
fn remote_window_arrival_plan_uses_authoritative_notification_and_retains_failures() {
    let transition = dispatch(
        &remote_action_snapshot(),
        json!({"surface_id": A, "action": "new-terminal-right"}),
    );
    let remote = serialized_effect(&transition, "RemoteCreate");
    assert_eq!(
        json!({
            "arrival_policy": remote["arrival_policy"],
            "observation_source": remote["observation_source"],
            "pending_reconciliation": remote["pending_reconciliation"],
            "observation_failure_policy": remote["observation_failure_policy"],
            "commit_failure_policy": remote["commit_failure_policy"],
            "fabricated_surface_id": remote["fabricated_surface_id"],
            "fabricated_pane_id": remote["fabricated_pane_id"]
        }),
        json!({
            "arrival_policy": "runtime-window-add",
            "observation_source": "tmux-new-window-output",
            "pending_reconciliation": true,
            "observation_failure_policy": "retain-pending-and-report",
            "commit_failure_policy": "retain-pending-and-retry",
            "fabricated_surface_id": null,
            "fabricated_pane_id": null
        })
    );
}

#[test]
fn unchanged_effect_only_actions_do_not_request_snapshot_persistence_or_publication() {
    let local = action_snapshot();
    let remote = remote_action_snapshot();
    let transitions = [
        (
            dispatch(&local, json!({"surface_id": B, "action": "reload"})),
            &local,
        ),
        (
            dispatch_with(
                &local,
                "surface.action",
                json!({
                    "surface_id": B,
                    "action": "new-browser-right",
                    "url": "https://external.test"
                }),
                &context(false),
            ),
            &local,
        ),
        (
            dispatch(
                &remote,
                json!({"surface_id": A, "action": "new-terminal-right"}),
            ),
            &remote,
        ),
    ];
    let mut actual_changed = Vec::new();
    for (transition, original) in transitions {
        assert!(!transition.effects.is_empty());
        assert_eq!(&transition.snapshot, original);
        actual_changed.push(transition.changed);
        assert!(!transition
            .effects
            .iter()
            .any(|effect| { matches!(effect, LifecycleEffect::PersistSession) }));
    }
    assert_eq!(
        actual_changed,
        vec![false, false, false],
        "reload, shell-open, and accepted remote create must not persist or publish session.changed"
    );
}

#[test]
fn pin_normalizes_the_pinned_prefix_before_later_range_and_insert_actions() {
    let snapshot = action_snapshot();
    let pinned = dispatch(&snapshot, json!({"surface_id": C, "action": "pin"}));
    assert_eq!(
        model(&pinned.snapshot).pane(P1).unwrap().surface_ids,
        vec![C, A, B]
    );

    let closed = dispatch(
        &pinned.snapshot,
        json!({"surface_id": B, "action": "close-left"}),
    );
    assert_eq!(ok(&closed)["skipped_pinned"], 1);
}

#[test]
fn foundation_equivalent_urls_include_non_http_and_relative_values() {
    for url in [
        "about:blank",
        "file:///C:/repo/readme.html",
        "docs/index.html",
    ] {
        let transition = dispatch(
            &action_snapshot(),
            json!({"surface_id": A, "action": "new-browser-right", "url": url}),
        );
        assert!(ok(&transition)["created_surface_id"].is_string(), "{url}");
    }

    let mut snapshot = action_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row.surface_id == B)
        .unwrap()
        .kind = browser("about:blank");
    let duplicate = dispatch(&snapshot, json!({"surface_id": B, "action": "duplicate"}));
    assert!(ok(&duplicate)["created_surface_id"].is_string());
}

#[test]
fn action_focus_uses_the_shared_v2_bool_parser() {
    for value in [
        json!(1),
        json!(2),
        json!(-1),
        json!(0.5),
        json!("yes"),
        json!("on"),
        json!("true"),
    ] {
        let transition = dispatch(
            &action_snapshot(),
            json!({"surface_id": A, "action": "new-terminal-right", "focus": value}),
        );
        let created = ok(&transition)["created_surface_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            model(&transition.snapshot).focused_surface(WS1),
            Some(created.as_str())
        );
    }
    let not_focused = dispatch(
        &action_snapshot(),
        json!({"surface_id": A, "action": "new-terminal-right", "focus": "off"}),
    );
    assert_eq!(model(&not_focused.snapshot).focused_surface(WS1), Some(A));
    let zero = dispatch(
        &action_snapshot(),
        json!({"surface_id": A, "action": "new-terminal-right", "focus": 0}),
    );
    assert_eq!(model(&zero.snapshot).focused_surface(WS1), Some(A));
}

#[test]
fn full_width_toggle_focuses_target_persists_and_has_exact_missing_pane_error() {
    let snapshot = action_snapshot();
    let toggled = dispatch(
        &snapshot,
        json!({"surface_id": B, "action": "toggle-full-width-tab"}),
    );
    assert_eq!(ok(&toggled)["full_width_tab_mode"], true);
    assert_eq!(model(&toggled.snapshot).focused_surface(WS1), Some(B));
    let restored = cmux_core::session::decode_session(
        &cmux_core::session::encode_session(&toggled.snapshot).unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored.windows[0].tab_manager.workspaces[0]
            .zoomed_panel_id
            .as_deref(),
        Some(B),
        "canonical full-width presentation state must survive restore"
    );

    let mut missing_pane = action_snapshot();
    missing_pane.windows[0].tab_manager.workspaces[0].layout = None;
    assert_error(
        &dispatch(
            &missing_pane,
            json!({"surface_id": B, "action": "toggle-full-width-tab"}),
        ),
        "not_found",
        "Tab pane not found",
    );
}

#[test]
fn focused_move_updates_both_selected_workspace_representations() {
    let transition = dispatch(
        &action_snapshot(),
        json!({"surface_id": B, "action": "move-to-new-workspace", "focus": true}),
    );
    let value = ok(&transition);
    let window = &transition.snapshot.windows[0];
    let selected = usize::try_from(window.tab_manager.selected_workspace_index.unwrap()).unwrap();
    assert_eq!(
        window.tab_manager.workspaces[selected]
            .workspace_id
            .as_deref(),
        value["workspace_id"].as_str()
    );
    assert_eq!(
        window.selected_workspace_id.as_deref(),
        value["workspace_id"].as_str()
    );
}

#[test]
fn action_create_effects_carry_exact_failure_mapping() {
    let terminal = dispatch(
        &action_snapshot(),
        json!({"surface_id": A, "action": "new-terminal-right"}),
    );
    let terminal_effect = serialized_effect(&terminal, "TerminalCreate");
    assert_eq!(terminal_effect["failure_code"], "internal_error");
    assert_eq!(terminal_effect["failure_message"], "Failed to create tab");

    let duplicate = dispatch(
        &action_snapshot(),
        json!({"surface_id": B, "action": "duplicate"}),
    );
    let browser_effect = serialized_effect(&duplicate, "BrowserAttach");
    assert_eq!(browser_effect["failure_message"], "Failed to duplicate tab");

    // Frozen canonical treats unsuccessful close-range members as unclosed,
    // continues the range, and reports the final counts.
}

#[test]
fn externally_visible_reload_and_shell_open_are_commit_phase_effects() {
    let reload = dispatch(
        &action_snapshot(),
        json!({"surface_id": B, "action": "reload"}),
    );
    assert_eq!(
        serialized_effect(&reload, "BrowserReload")["phase"],
        "commit"
    );

    let opened = dispatch_with(
        &action_snapshot(),
        "surface.action",
        json!({
            "surface_id": B,
            "action": "new-browser-right",
            "url": "https://external.test"
        }),
        &context(false),
    );
    assert_eq!(
        serialized_effect(&opened, "ExternalBrowserOpen")["phase"],
        "commit"
    );
}

#[test]
fn completion_and_error_payloads_match_final_reference_decoration() {
    let transition = dispatch(
        &action_snapshot(),
        json!({"surface_id": A, "action": "new-terminal-right"}),
    );
    let raw_result = ok(&transition);
    let completion = transition
        .events
        .iter()
        .find(|event| event.name == "surface.action")
        .unwrap();
    assert_eq!(completion.payload["result"], raw_result);

    let mut registry = ControlHandleRegistry::default();
    let mut decorated_result = raw_result;
    decorate_lifecycle_value_refs(&mut decorated_result, &mut |kind, id| {
        registry.mint(kind, id)
    });
    let mut decorated_completion = completion.payload["result"].clone();
    decorate_lifecycle_value_refs(&mut decorated_completion, &mut |kind, id| {
        registry.mint(kind, id)
    });
    assert_eq!(decorated_completion, decorated_result);

    let missing = "40000000-0000-0000-0000-000000000099";
    let failed = dispatch(
        &action_snapshot(),
        json!({"surface_id": missing, "action": "pin"}),
    );
    let mut data = assert_error(&failed, "not_found", "Tab not found");
    decorate_lifecycle_value_refs(&mut data, &mut |kind, id| registry.mint(kind, id));
    assert!(data["surface_ref"].is_string());
    assert!(data["tab_ref"].is_string());
}

#[test]
fn browser_disabled_completion_uses_null_result_owners() {
    let transition = dispatch_with(
        &action_snapshot(),
        "surface.action",
        json!({
            "surface_id": B,
            "action": "new-browser-right",
            "url": "https://external.test"
        }),
        &context(false),
    );
    let completion = transition
        .events
        .iter()
        .find(|event| event.name == "surface.action")
        .unwrap();
    assert_eq!(completion.window_id.as_deref(), Some(W1));
    assert_eq!(completion.workspace_id, None);
    assert_eq!(completion.pane_id, None);
    assert_eq!(completion.surface_id, None);
}
