use super::pane_surface_lifecycle::{
    commit_lifecycle_transition, dispatch_lifecycle_request, reconcile_runtime_arrival,
    LifecycleDispatchContext, LifecycleEffect, LifecycleEffectExecutor, LifecycleTransition,
    RuntimeArrival,
};
use super::*;
use crate::dock::{
    DockCreateRequest, DockRuntimeIntent, DockRuntimeOperation, DockStore, DockSurfaceKind,
};
use cmux_core::surface_lifecycle::{AttachOutcome, RuntimeHandle, SurfaceLifecycleModel};
use cmux_core::{
    session::{decode_session, encode_session},
    surface_lifecycle::ContainerKind,
};

const V2_LIFECYCLE_METHODS: [&str; 11] = [
    "pane.create",
    "pane.resize",
    "surface.action",
    "surface.create",
    "surface.current",
    "surface.list",
    "surface.report_pwd",
    "surface.respawn",
    "surface.close",
    "surface.focus",
    "surface.move",
];

fn two_window_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].selected_workspace_id = Some("workspace-1".into());
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-1".into());

    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("window-2".into());
    second.selected_workspace_id = Some("workspace-2".into());
    second.tab_manager.selected_workspace_index = Some(0);
    second.tab_manager.workspaces[0].workspace_id = Some("workspace-2".into());
    second.tab_manager.workspaces[0].focused_panel_id = Some("surface-2".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second.tab_manager.workspaces[0]
        .layout
        .as_mut()
        .expect("second window layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2".into());
    pane.panel_ids = vec!["surface-2".into()];
    pane.selected_panel_id = Some("surface-2".into());
    snapshot.windows.push(second);
    snapshot
}

fn mark_remote_tmux_workspace(
    snapshot: AppSessionSnapshot,
    window_index: usize,
) -> AppSessionSnapshot {
    let mut encoded = serde_json::to_value(snapshot).expect("encode remote fixture");
    encoded["windows"][window_index]["tab_manager"]["workspaces"][0]["remote"] = json!({
        "enabled": true,
        "state": "connected",
        "connected": true,
        "transport": "tmux",
        "destination": "remote-session-1"
    });
    serde_json::from_value(encoded).expect("decode remote fixture")
}

fn intended_mixed_surface_records() -> Value {
    json!([
        {
            "surface_id": "surface-terminal",
            "pane_id": "pane-mixed",
            "generation": 7,
            "kind": {"type": "terminal"},
            "metadata": {
                "custom_title": "Build shell",
                "pinned": true,
                "reported_directory": "C:/repo/terminal"
            },
            "terminal_startup": {
                "command": "cargo test",
                "working_directory": "C:/repo/terminal"
            }
        },
        {
            "surface_id": "surface-browser",
            "pane_id": "pane-mixed",
            "generation": 3,
            "kind": {
                "type": "browser",
                "url": "https://example.test",
                "developer_tools_visible": true
            },
            "metadata": {
                "custom_title": "Docs",
                "unread": true
            }
        }
    ])
}

fn mixed_surface_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-terminal".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("mixed surface layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-mixed".into());
    pane.panel_ids = vec!["surface-terminal".into(), "surface-browser".into()];
    pane.selected_panel_id = Some("surface-browser".into());
    pane.surface_kind = None;
    pane.browser_url = None;
    pane.browser_developer_tools_visible = None;

    let mut encoded = serde_json::to_value(snapshot).expect("encode mixed fixture");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] =
        intended_mixed_surface_records();
    serde_json::from_value(encoded).expect("decode mixed fixture")
}

fn resizable_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-left".into());
    let SessionWorkspaceLayoutSnapshot::Pane(base) = workspace.layout.take().expect("base pane")
    else {
        unreachable!();
    };
    let mut left = base.clone();
    left.pane_id = Some("pane-left".into());
    left.panel_ids = vec!["surface-left".into()];
    left.selected_panel_id = Some("surface-left".into());
    let mut right = base;
    right.pane_id = Some("pane-right".into());
    right.panel_ids = vec!["surface-right".into()];
    right.selected_panel_id = Some("surface-right".into());
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Split(
        cmux_core::session::SessionSplitLayoutSnapshot {
            split_id: Some("split-root".into()),
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(left)),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(right)),
        },
    ));
    snapshot
}

fn context() -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1_000.0, 800.0)),
        browser_enabled: true,
        dock_available: true,
        active_window_id: None,
    }
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        method,
        params.as_object().expect("decoded params object"),
        &context(),
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

fn main_window_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    snapshot
}

fn main_dock_snapshot() -> (AppSessionSnapshot, String, String) {
    let mut snapshot = main_window_snapshot();
    let created = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                title: Some("Dock shell".into()),
                working_directory: Some("C:/repo".into()),
                command: Some("cargo test".into()),
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .expect("seed Dock surface");
    (
        snapshot,
        created.pane_id.to_string(),
        created.surface_id.to_string(),
    )
}

fn main_context(dock_available: bool, browser_enabled: bool) -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1_000.0, 800.0)),
        browser_enabled,
        dock_available,
        active_window_id: Some("main".into()),
    }
}

fn main_transition(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: Value,
    dock_available: bool,
    browser_enabled: bool,
) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        method,
        params.as_object().expect("decoded params object"),
        &main_context(dock_available, browser_enabled),
    )
}

#[derive(Default)]
struct RecordingExecutor {
    staged: Vec<LifecycleEffect>,
    committed: Vec<LifecycleEffect>,
    fail_stage_at: Option<usize>,
    rollback_count: usize,
}

impl LifecycleEffectExecutor for RecordingExecutor {
    type Error = String;

    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error> {
        if self.fail_stage_at == Some(self.staged.len()) {
            return Err("injected effect failure".into());
        }
        self.staged.push(effect.clone());
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        self.committed.append(&mut self.staged);
        Ok(())
    }

    fn rollback_staged(&mut self) -> Result<(), Self::Error> {
        self.rollback_count += 1;
        self.staged.clear();
        Ok(())
    }
}

#[test]
fn lifecycle_registry_binds_every_public_method_to_the_production_route() {
    for method in V2_LIFECYCLE_METHODS {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
        assert_eq!(
            control_request_route_for_method(method),
            ControlRequestRoute::PaneSurfaceLifecycle,
            "{method} bypasses the shared production lifecycle dispatcher"
        );
    }
    for dependency in ["pane.focus", "surface.split", "tab.action"] {
        assert_eq!(
            control_request_route_for_method(dependency),
            ControlRequestRoute::PaneSurfaceLifecycle,
            "{dependency} must mutate the authoritative lifecycle model"
        );
    }
    assert!(CONTROL_SOCKET_METHODS.contains(&"tab.action"));
    assert_eq!(
        control_request_route_for_method("workspace.rename"),
        ControlRequestRoute::Legacy
    );
}

#[test]
fn staged_effect_executor_rolls_back_before_any_external_effect_becomes_visible() {
    let mut snapshot = test_snapshot();
    let before = snapshot.clone();
    let planned = transition(
        &snapshot,
        "pane.create",
        json!({
            "direction": "right",
            "type": "browser",
            "url": "https://rollback.test",
            "focus": true
        }),
    );
    assert!(planned
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::BrowserAttach { .. })));
    assert!(planned
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::ActivateWindow { .. })));
    assert!(planned.effects.len() >= 2);

    let mut executor = RecordingExecutor {
        fail_stage_at: Some(1),
        ..RecordingExecutor::default()
    };
    assert!(commit_lifecycle_transition(&mut snapshot, planned, &mut executor).is_err());
    assert_eq!(
        snapshot, before,
        "failed effects must not commit model state"
    );
    assert_eq!(
        executor.rollback_count, 1,
        "a later staging failure must compensate the already-staged effect"
    );
    assert!(executor.staged.is_empty());
    assert!(
        executor.committed.is_empty(),
        "staged effects must not become externally visible after validation fails"
    );
}

#[test]
fn pane_create_preserves_exact_validation_and_committed_event_contract() {
    let snapshot = test_snapshot();
    let invalid = transition(
        &snapshot,
        "pane.create",
        json!({"direction": "diagonal", "placement": "somewhere"}),
    );
    assert_error(
        &invalid,
        "invalid_params",
        "Missing or invalid direction (left|right|up|down)",
    );
    assert!(!invalid.changed);
    assert!(invalid.events.is_empty());
    assert!(invalid.effects.is_empty());

    let created = transition(
        &snapshot,
        "pane.create",
        json!({
            "direction": "right",
            "type": "browser",
            "url": "https://example.test",
            "initial_divider_position": 0.75,
            "focus": false
        }),
    );
    let value = ok_value(&created);
    assert_eq!(value["type"], json!("browser"));
    assert!(value["pane_id"].is_string());
    assert!(value["surface_id"].is_string());
    assert_eq!(
        created
            .events
            .iter()
            .filter(|event| event.name == "pane.created")
            .count(),
        1
    );
    assert_eq!(
        created
            .events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );
    assert!(created.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::BrowserAttach { surface_id, .. }
            if Some(surface_id.as_str()) == value["surface_id"].as_str()
    )));
    assert_eq!(
        created.snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id,
        snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id
    );
}

#[test]
fn lifecycle_events_carry_complete_owner_envelopes() {
    let created = transition(
        &test_snapshot(),
        "pane.create",
        json!({"direction":"right","type":"browser","url":"https://events.test"}),
    );
    let value = ok_value(&created);
    for event in &created.events {
        assert_eq!(event.source, "workspace.lifecycle");
        assert!(matches!(event.category, "pane" | "surface"));
        assert_eq!(event.window_id.as_deref(), Some("window-1"));
        assert_eq!(event.workspace_id.as_deref(), Some("workspace-1"));
        assert_eq!(event.pane_id.as_deref(), value["pane_id"].as_str());
        assert_eq!(event.surface_id.as_deref(), value["surface_id"].as_str());
        assert_eq!(event.payload["pane_id"], value["pane_id"]);
        assert_eq!(event.payload["surface_id"], value["surface_id"]);
        assert!(event.payload.get("window_id").is_none());
        assert!(event.payload.get("workspace_id").is_none());
        assert_eq!(event.payload["origin"], "browser_split");
    }

    let action = transition(
        &created.snapshot,
        "surface.action",
        json!({"surface_id":value["surface_id"],"action":"pin"}),
    );
    let completion = action
        .events
        .iter()
        .find(|event| event.name == "surface.action")
        .unwrap();
    assert_eq!(completion.source, "socket.v2");
    assert_eq!(completion.window_id.as_deref(), Some("window-1"));
    assert_eq!(completion.workspace_id.as_deref(), Some("workspace-1"));
    assert_eq!(completion.pane_id.as_deref(), value["pane_id"].as_str());
    assert_eq!(
        completion.surface_id.as_deref(),
        value["surface_id"].as_str()
    );
    assert_eq!(completion.payload["method"], "surface.action");
    assert_eq!(completion.payload["params"]["action"], "pin");
    assert_eq!(completion.payload["result"]["pinned"], true);
}

#[test]
fn pane_resize_uses_injected_dimensions_and_absolute_validation_precedence() {
    let snapshot = resizable_snapshot();
    let invalid = transition(
        &snapshot,
        "pane.resize",
        json!({
            "absolute_axis": "diagonal",
            "target_pixels": 640,
            "direction": "left",
            "amount": 2
        }),
    );
    assert_error(
        &invalid,
        "invalid_params",
        "absolute_axis must be 'horizontal' or 'vertical'",
    );
    assert!(!invalid.changed);

    let resized = transition(
        &snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "absolute_axis": "horizontal", "target_pixels": 600}),
    );
    let value = ok_value(&resized);
    assert_eq!(value["split_id"], json!("split-root"));
    assert_eq!(value["old_divider_position"], json!(0.5));
    assert_eq!(value["new_divider_position"], json!(0.6));
    assert_eq!(
        resized
            .events
            .iter()
            .filter(|event| event.name == "pane.resized")
            .count(),
        1
    );
}

#[test]
fn surface_create_and_action_use_authoritative_records_and_typed_effects() {
    let snapshot = test_snapshot();
    let invalid = transition(
        &snapshot,
        "surface.create",
        json!({"pane_id": "pane-1", "type": "agentSession", "provider": "bogus"}),
    );
    assert_eq!(
        assert_error(
            &invalid,
            "invalid_params",
            "Invalid provider (codex|claude|opencode)"
        ),
        json!({"provider": "bogus"})
    );

    let created = transition(
        &snapshot,
        "surface.create",
        json!({
            "pane_id": "pane-1",
            "type": "terminal",
            "working_directory": "C:/created",
            "initial_command": "cargo test",
            "focus": false
        }),
    );
    let value = ok_value(&created);
    let created_id = value["surface_id"].as_str().unwrap();
    assert!(created.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::TerminalCreate { surface_id, .. } if surface_id == created_id
    )));

    let renamed = transition(
        &created.snapshot,
        "surface.action",
        json!({"surface_id": created_id, "action": "rename", "title": " Build logs "}),
    );
    let renamed_value = ok_value(&renamed);
    assert_eq!(renamed_value["title"], json!("Build logs"));
    assert_eq!(
        renamed
            .events
            .iter()
            .filter(|event| event.name == "surface.action")
            .count(),
        1
    );
}

#[test]
fn dock_and_remote_create_require_real_typed_effects_not_counter_payloads() {
    let dock = transition(
        &test_snapshot(),
        "surface.create",
        json!({"placement": "dock", "type": "browser", "url": "https://dock.test", "focus": false}),
    );
    let value = ok_value(&dock);
    assert_eq!(value["placement"], json!("dock"));
    assert!(value["surface_id"].is_null());
    assert!(value["dock_surface_id"].is_string());
    assert!(dock.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::DockCreate { dock_surface_id, .. }
            if Some(dock_surface_id.as_str()) == value["dock_surface_id"].as_str()
    )));

    let ignored_request_metadata = transition(
        &test_snapshot(),
        "pane.create",
        json!({
            "direction": "right",
            "remote_pty_session_id": "remote-session-1",
            "remote_context": "cloud"
        }),
    );
    let value = ok_value(&ignored_request_metadata);
    assert!(value.get("routed").is_none());
    assert!(value["pane_id"].is_string());
    assert!(!ignored_request_metadata
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })));

    let mut remote_snapshot = mark_remote_tmux_workspace(test_snapshot(), 0);
    let mut lifecycle = SurfaceLifecycleModel::from_app_session_snapshot(&remote_snapshot).unwrap();
    lifecycle
        .replace_kind(
            "surface-1",
            SessionSurfaceKindSnapshot::RemoteTerminal {
                remote_session_id: Some("%1".into()),
                remote_context: None,
                arrival_generation: Some(1),
            },
        )
        .unwrap();
    remote_snapshot = lifecycle.to_app_session(&remote_snapshot).unwrap();
    let remote = transition(
        &remote_snapshot,
        "pane.create",
        json!({"direction": "right"}),
    );
    let value = ok_value(&remote);
    assert_eq!(value["accepted"], json!(true));
    assert_eq!(value["routed"], json!("remote-tmux"));
    assert!(value["pane_id"].is_null());
    assert!(remote.events.is_empty(), "arrival owns lifecycle events");
    assert!(remote.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::RemoteCreate { remote_session_id, .. }
            if remote_session_id == "remote-session-1"
    )));
}

#[test]
fn current_and_list_run_through_shared_dispatch_without_focus_side_effects() {
    let snapshot = mixed_surface_snapshot();
    let current = transition(
        &snapshot,
        "surface.current",
        json!({"surface_id": "surface-browser"}),
    );
    let current_value = ok_value(&current);
    assert_eq!(current_value["surface_id"], json!("surface-terminal"));
    assert_eq!(current_value["surface_type"], json!("terminal"));
    assert!(!current.changed);
    assert!(current.events.is_empty());
    assert!(current.effects.is_empty());
    assert_eq!(current.snapshot, snapshot);

    let listed = transition(&snapshot, "surface.list", json!({}));
    let value = ok_value(&listed);
    let rows = value["surfaces"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["index"], json!(0));
    assert_eq!(rows[0]["index_in_pane"], json!(0));
    assert_eq!(rows[0]["type"], json!("terminal"));
    assert_eq!(rows[0]["focused"], json!(true));
    assert_eq!(rows[0]["selected_in_pane"], json!(false));
    assert!(rows[0].get("requested_working_directory").is_some());
    assert!(rows[0].get("resume_binding").is_some());
    assert!(rows[0].get("developer_tools_visible").is_none());
    assert_eq!(rows[1]["type"], json!("browser"));
    assert_eq!(rows[1]["focused"], json!(false));
    assert_eq!(rows[1]["selected_in_pane"], json!(true));
    assert_eq!(rows[1]["developer_tools_visible"], json!(true));
    assert!(rows[1].get("requested_working_directory").is_none());
}

#[test]
fn surface_focus_requires_an_identity_and_commits_workspace_focus() {
    let snapshot = mixed_surface_snapshot();
    let invalid = transition(&snapshot, "surface.focus", json!({}));
    assert_error(&invalid, "invalid_params", "Missing or invalid surface_id");
    assert!(!invalid.changed);

    let focused = transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id": "surface-browser"}),
    );
    let value = ok_value(&focused);
    assert_eq!(value["workspace_id"], json!("workspace-1"));
    assert_eq!(value["surface_id"], json!("surface-browser"));
    assert!(focused.changed);
    assert_eq!(
        focused.snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-browser")
    );
    assert!(focused.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::ActivateWindow { window_id } if window_id == "window-1"
    )));
}

#[test]
fn explicit_second_window_routes_by_identity_without_assuming_handle_ref_number() {
    let snapshot = two_window_snapshot();
    let listed = transition(&snapshot, "surface.list", json!({"window_id": "window-2"}));
    let value = ok_value(&listed);
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["surfaces"][0]["id"], json!("surface-2"));

    let invalid = transition(
        &snapshot,
        "surface.list",
        json!({"window_id": "missing-window", "workspace_id": "workspace-2"}),
    );
    assert_error(&invalid, "unavailable", "TabManager not available");
    assert!(!invalid.changed);
}

#[test]
fn report_pwd_preserves_raw_path_and_reconciles_pending_remote_once() {
    let snapshot = test_snapshot();
    let conflict = transition(
        &snapshot,
        "surface.report_pwd",
        json!({"workspace_id": "workspace-1", "path": "C:/one", "directory": "C:/two"}),
    );
    assert_error(&conflict, "invalid_params", "Conflicting path parameters");

    let recorded = transition(
        &snapshot,
        "surface.report_pwd",
        json!({
            "workspace_id": "workspace-1",
            "surface_id": "surface-1",
            "path": " C:/repo ",
            "cwd": " C:/repo "
        }),
    );
    assert_eq!(ok_value(&recorded)["path"], json!(" C:/repo "));
    assert!(recorded.events.is_empty());

    let mut remote = mark_remote_tmux_workspace(two_window_snapshot(), 1);
    remote.windows[1].tab_manager.workspaces[0].layout = None;
    remote.windows[1].tab_manager.workspaces[0].surfaces = Some(Vec::new());
    remote.windows[1].tab_manager.workspaces[0].focused_panel_id = None;
    let pending = transition(
        &remote,
        "surface.report_pwd",
        json!({
            "workspace_id": "workspace-2",
            "surface_id": "arriving-surface",
            "path": "C:/remote"
        }),
    );
    assert_eq!(ok_value(&pending)["pending"], json!(true));
    let first = reconcile_runtime_arrival(
        &pending.snapshot,
        RuntimeArrival::remote(
            "window-2",
            "workspace-2",
            "pane-remote",
            "arriving-surface",
            "remote-42",
            1,
        ),
    );
    let second = reconcile_runtime_arrival(
        &first.snapshot,
        RuntimeArrival::remote(
            "window-2",
            "workspace-2",
            "pane-remote",
            "arriving-surface",
            "remote-42",
            1,
        ),
    );
    assert_eq!(first.directory_apply_count("arriving-surface"), 1);
    assert_eq!(second.directory_apply_count("arriving-surface"), 1);
}

#[test]
fn runtime_arrival_preserves_nonempty_topology_and_fences_duplicate_and_stale_callbacks() {
    let remote = mark_remote_tmux_workspace(two_window_snapshot(), 1);
    let pending = transition(
        &remote,
        "surface.report_pwd",
        json!({"workspace_id":"workspace-2","surface_id":"arriving-surface","path":"C:/remote"}),
    );
    let arrival = RuntimeArrival::remote(
        "window-2",
        "workspace-2",
        "pane-remote",
        "arriving-surface",
        "remote-42",
        1,
    );
    let first = reconcile_runtime_arrival(&pending.snapshot, arrival.clone());
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&first.snapshot).unwrap();
    assert!(model.surface("surface-2").is_some());
    assert!(model.surface("arriving-surface").is_some());
    assert_eq!(
        model.owner_of_surface("surface-2").unwrap().pane_id,
        "pane-2"
    );
    assert_eq!(
        model.owner_of_surface("arriving-surface").unwrap().pane_id,
        "pane-remote"
    );
    assert_eq!(first.directory_apply_count("arriving-surface"), 1);
    assert_eq!(
        reconcile_runtime_arrival(&first.snapshot, arrival).snapshot,
        first.snapshot
    );
    let stale = RuntimeArrival::remote(
        "window-2",
        "workspace-2",
        "pane-remote",
        "arriving-surface",
        "remote-42",
        0,
    );
    assert_eq!(
        reconcile_runtime_arrival(&first.snapshot, stale).snapshot,
        first.snapshot
    );
}

#[test]
fn respawn_and_close_emit_typed_runtime_replace_and_teardown_effects() {
    let snapshot = mixed_surface_snapshot();
    let invalid = transition(
        &snapshot,
        "surface.respawn",
        json!({"surface_id": "surface-terminal", "focus": "maybe"}),
    );
    assert_error(&invalid, "invalid_params", "Missing or invalid focus");

    let respawned = transition(
        &snapshot,
        "surface.respawn",
        json!({
            "surface_id": "surface-terminal",
            "command": "  cargo test  ",
            "initial_command": "ignored",
            "working_directory": "C:/respawn",
            "focus": false
        }),
    );
    assert_eq!(
        ok_value(&respawned)["surface_id"],
        json!("surface-terminal")
    );
    assert!(respawned.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::TerminalReplace {
            surface_id,
            previous_generation: 7,
            generation: 8,
            ..
        } if surface_id == "surface-terminal"
    )));
    assert!(!respawned
        .events
        .iter()
        .any(|event| { matches!(event.name, "surface.created" | "surface.closed") }));

    let closed = transition(
        &respawned.snapshot,
        "surface.close",
        json!({"surface_id": "surface-browser"}),
    );
    assert_eq!(ok_value(&closed)["surface_id"], json!("surface-browser"));
    assert!(closed.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::RuntimeTeardown { surface_id, .. } if surface_id == "surface-browser"
    )));
    assert_eq!(
        closed
            .events
            .iter()
            .filter(|event| event.name == "surface.closed")
            .count(),
        1
    );
}

#[test]
fn global_model_fences_stale_runtime_attach_across_the_app_snapshot() {
    let snapshot = two_window_snapshot();
    let mut model = SurfaceLifecycleModel::from_app_session_snapshot(&snapshot).unwrap();
    let generation = model.surface("surface-2").unwrap().generation;
    assert_eq!(
        model.attach_runtime(
            "surface-2",
            generation.saturating_sub(1),
            RuntimeHandle::new("stale-runtime")
        ),
        AttachOutcome::StaleCleaned
    );
    assert!(model.owner_of_runtime("stale-runtime").is_none());
    assert_eq!(
        model.attach_runtime("surface-2", generation, RuntimeHandle::new("live-runtime")),
        AttachOutcome::Attached
    );
    assert_eq!(
        model.owner_of_runtime("live-runtime").unwrap().window_id,
        "window-2"
    );
}

#[test]
fn move_is_one_global_two_window_transaction_and_preserves_metadata() {
    let mut snapshot = two_window_snapshot();
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([{
        "surface_id": "surface-1",
        "pane_id": "pane-1",
        "generation": 4,
        "kind": {"type": "terminal"},
        "metadata": {"custom_title": "Mover", "pinned": true}
    }]);
    snapshot = serde_json::from_value(encoded).unwrap();

    let moved = transition(
        &snapshot,
        "surface.move",
        json!({"surface_id": "surface-1", "window_id": "window-2", "focus": true}),
    );
    let value = ok_value(&moved);
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["pane_id"], json!("pane-2"));
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&moved.snapshot).unwrap();
    let owner = model.owner_of_surface("surface-1").unwrap();
    assert_eq!(owner.window_id, "window-2");
    assert_eq!(owner.workspace_id, "workspace-2");
    assert_eq!(
        model
            .surface("surface-1")
            .unwrap()
            .metadata
            .custom_title
            .as_deref(),
        Some("Mover")
    );
    assert!(model.surface("surface-1").unwrap().metadata.pinned);
    assert_eq!(
        moved
            .events
            .iter()
            .filter(|event| event.name == "surface.moved")
            .count(),
        1
    );
}

#[test]
fn restore_dispatch_serialize_restore_preserves_authoritative_state() {
    let snapshot = mixed_surface_snapshot();
    let renamed = transition(
        &snapshot,
        "surface.action",
        json!({"surface_id": "surface-browser", "action": "rename", "title": "Restored docs"}),
    );
    let bytes = cmux_core::session::encode_session(&renamed.snapshot).unwrap();
    let restored = cmux_core::session::decode_session(&bytes).unwrap();
    let listed = transition(&restored, "surface.list", json!({}));
    let rows = ok_value(&listed)["surfaces"].as_array().unwrap().clone();
    let browser = rows
        .iter()
        .find(|row| row["id"] == "surface-browser")
        .unwrap();
    assert_eq!(browser["title"], json!("Restored docs"));
    assert_eq!(browser["developer_tools_visible"], json!(true));
    assert!(
        serde_json::to_value(restored).unwrap()["windows"][0]["tab_manager"]["workspaces"][0]
            ["panel_titles"]
            .is_null()
    );
}

#[test]
fn action_matrix_and_close_range_skip_pinned_and_preserve_last_surface() {
    let snapshot = mixed_surface_snapshot();
    let pinned = transition(
        &snapshot,
        "surface.action",
        json!({"surface_id": "surface-browser", "action": "pin"}),
    );
    assert_eq!(ok_value(&pinned)["pinned"], json!(true));
    let unread = transition(
        &pinned.snapshot,
        "surface.action",
        json!({"surface_id": "surface-browser", "action": "mark-unread"}),
    );
    assert_eq!(ok_value(&unread)["action"], json!("mark_unread"));
    let close_right = transition(
        &unread.snapshot,
        "surface.action",
        json!({"surface_id": "surface-terminal", "action": "close-right"}),
    );
    let value = ok_value(&close_right);
    assert_eq!(value["closed"], json!(0));
    assert_eq!(value["skipped_pinned"], json!(1));

    let close_last = transition(
        &snapshot,
        "surface.close",
        json!({"surface_id": "surface-browser"}),
    );
    let last = transition(
        &close_last.snapshot,
        "surface.close",
        json!({"surface_id": "surface-terminal"}),
    );
    assert_error(&last, "invalid_state", "Cannot close the last surface");
    assert!(!last.changed);
}

#[test]
fn pane_focus_and_surface_split_regressions_mutate_authoritative_records() {
    let snapshot = resizable_snapshot();
    let focused = transition(
        &snapshot,
        "pane.focus",
        json!({"workspace_id": "workspace-1", "pane_id": "pane-right"}),
    );
    assert_eq!(
        focused.snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-right")
    );
    SurfaceLifecycleModel::from_app_session_snapshot(&focused.snapshot)
        .unwrap()
        .validate_indexes()
        .unwrap();

    let split = transition(
        &focused.snapshot,
        "surface.split",
        json!({"surface_id": "surface-right", "direction": "l", "type": "Browser", "focus": true}),
    );
    let value = ok_value(&split);
    let created_id = value["surface_id"].as_str().unwrap();
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&split.snapshot).unwrap();
    assert!(matches!(
        model.surface(created_id).unwrap().kind,
        cmux_core::surface_lifecycle::SurfaceKind::Browser { .. }
    ));
    assert_eq!(model.focused_surface("workspace-1"), Some(created_id));
    model.validate_indexes().unwrap();
}

#[test]
fn persisted_schema_has_one_per_surface_authority() {
    let encoded = serde_json::to_value(mixed_surface_snapshot()).unwrap();
    let workspace = &encoded["windows"][0]["tab_manager"]["workspaces"][0];
    assert_eq!(workspace["surfaces"], intended_mixed_surface_records());
    for obsolete in [
        "panel_titles",
        "panel_pins",
        "panel_unreads",
        "panel_terminal_startups",
        "restorable_agent_snapshots",
    ] {
        assert!(
            workspace.get(obsolete).is_none(),
            "workspace retained {obsolete}"
        );
    }
    let pane = &workspace["layout"]["pane"];
    for obsolete in [
        "surface_kind",
        "browser_url",
        "browser_proxy_url",
        "browser_back_history",
        "browser_forward_history",
        "browser_developer_tools_visible",
        "markdown_file_path",
        "file_path",
        "diff_viewer_token",
        "diff_viewer_request_path",
    ] {
        assert!(pane.get(obsolete).is_none(), "pane retained {obsolete}");
    }
}

#[test]
fn dock_api_create_returns_owner_and_dock_scoped_identities_and_commits_runtime_state() {
    for (kind, url) in [("terminal", None), ("browser", Some("https://dock.test"))] {
        let mut params = json!({
            "placement": "dock",
            "type": kind,
            "focus": false,
        });
        if let Some(url) = url {
            params["url"] = json!(url);
        }
        let transition = main_transition(
            &main_window_snapshot(),
            "surface.create",
            params,
            true,
            true,
        );
        let value = ok_value(&transition);
        assert_eq!(value["window_id"], "main");
        assert_eq!(value["workspace_id"], "main");
        assert_eq!(value["placement"], "dock");
        assert_eq!(value["pane_id"], Value::Null);
        assert_eq!(value["pane_ref"], Value::Null);
        assert_eq!(value["surface_id"], Value::Null);
        assert_eq!(value["surface_ref"], Value::Null);
        let dock_pane_id = value["dock_pane_id"].as_str().expect("Dock pane identity");
        let dock_surface_id = value["dock_surface_id"]
            .as_str()
            .expect("Dock surface identity");
        assert!(Uuid::parse_str(dock_pane_id).is_ok());
        assert!(Uuid::parse_str(dock_surface_id).is_ok());
        assert_eq!(value["type"], kind);
        assert!(transition.effects.iter().any(|effect| matches!(
            effect,
            LifecycleEffect::DockCreate { dock_surface_id: effect_id, kind: effect_kind, .. }
                if effect_id == dock_surface_id && effect_kind == kind
        )));
        assert!(!transition
            .effects
            .iter()
            .any(|effect| matches!(effect, LifecycleEffect::ActivateWindow { .. })));
        assert!(!transition
            .effects
            .iter()
            .any(|effect| matches!(effect, LifecycleEffect::DockReveal { .. })));
        assert_eq!(
            transition
                .effects
                .iter()
                .filter(|effect| matches!(effect, LifecycleEffect::DockChanged { .. }))
                .count(),
            1
        );

        let model = SurfaceLifecycleModel::from_app_session(&transition.snapshot).unwrap();
        let record = model
            .surface(dock_surface_id)
            .expect("runtime identity is published into the authoritative snapshot");
        assert_eq!(record.generation, 1);
        let owner = model.owner_of_surface(dock_surface_id).unwrap();
        assert_eq!(owner.window_id, "main");
        assert_eq!(owner.workspace_id, "dock:main");
        assert_eq!(owner.pane_id, dock_pane_id);
        assert_eq!(
            model.pane(dock_pane_id).unwrap().container,
            ContainerKind::Dock
        );
        let dock = DockStore.snapshot(&transition.snapshot, "main");
        let dock_surface = dock
            .surfaces
            .iter()
            .find(|surface| surface.surface_id.to_string() == dock_surface_id)
            .unwrap();
        assert_eq!(dock_surface.generation, 1);
        match (&dock_surface.runtime, kind) {
            (DockRuntimeIntent::Terminal { .. }, "terminal")
            | (DockRuntimeIntent::Browser { .. }, "browser") => {}
            _ => panic!("Dock runtime intent did not match {kind}"),
        }

        let event = transition
            .events
            .iter()
            .find(|event| event.name == "surface.created")
            .expect("one Dock surface.created event");
        assert_eq!(event.window_id.as_deref(), Some("main"));
        assert_eq!(event.workspace_id.as_deref(), Some("main"));
        assert_eq!(event.pane_id.as_deref(), Some(dock_pane_id));
        assert_eq!(event.surface_id.as_deref(), Some(dock_surface_id));
        assert_eq!(
            transition
                .events
                .iter()
                .filter(|candidate| candidate.name == "surface.created")
                .count(),
            1
        );

        let restored = decode_session(&encode_session(&transition.snapshot).unwrap()).unwrap();
        let restored_model = SurfaceLifecycleModel::from_app_session(&restored).unwrap();
        restored_model.validate_indexes().unwrap();
        assert_eq!(
            restored_model.surface(dock_surface_id).unwrap().generation,
            1
        );
    }
}

#[test]
fn dock_api_validation_precedes_browser_fallback_and_rejects_non_dock_kinds() {
    for method in ["surface.create", "pane.create"] {
        let mut invalid_params = json!({
            "placement": "not-a-place",
            "type": "browser",
            "url": "https://example.test",
        });
        if method == "pane.create" {
            invalid_params["direction"] = json!("right");
        }
        let invalid = main_transition(
            &main_window_snapshot(),
            method,
            invalid_params,
            false,
            false,
        );
        assert_eq!(
            assert_error(
                &invalid,
                "invalid_params",
                "placement must be one of: workspace, dock"
            ),
            json!({"placement":"not-a-place"})
        );

        let mut disabled_params = json!({"placement":"dock", "type":"browser"});
        if method == "pane.create" {
            disabled_params["direction"] = json!("right");
        }
        let disabled = main_transition(
            &main_window_snapshot(),
            method,
            disabled_params,
            false,
            false,
        );
        assert_eq!(
            assert_error(&disabled, "invalid_params", "Dock placement is disabled"),
            json!({"placement":"dock"})
        );

        let mut unsupported_params = json!({"placement":"dock", "type":"markdown"});
        if method == "pane.create" {
            unsupported_params["direction"] = json!("down");
        }
        let unsupported = main_transition(
            &main_window_snapshot(),
            method,
            unsupported_params,
            true,
            true,
        );
        assert_eq!(
            assert_error(
                &unsupported,
                "invalid_params",
                "Dock placement supports only terminal and browser surfaces"
            ),
            json!({"type":"markdown"})
        );
    }
}

#[test]
fn remote_window_arrival_inserts_a_tab_right_of_non_last_source_without_a_pane_event() {
    let snapshot = mixed_surface_snapshot();
    let arrival = RuntimeArrival::remote_tab(
        "window-1",
        "workspace-1",
        "pane-mixed",
        "surface-observed",
        "%42",
        1,
        "surface-terminal",
        false,
    );
    assert_eq!(
        runtime_arrival_event_semantics(&arrival),
        (false, "terminal_tab")
    );

    let reconciled = reconcile_runtime_arrival(&snapshot, arrival);
    let workspace = &reconciled.snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        unreachable!()
    };
    assert_eq!(
        pane.panel_ids,
        ["surface-terminal", "surface-observed", "surface-browser"]
    );
    assert_eq!(
        workspace.focused_panel_id.as_deref(),
        Some("surface-terminal")
    );
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&reconciled.snapshot).unwrap();
    assert_eq!(
        model.owner_of_surface("surface-observed").unwrap().pane_id,
        "pane-mixed"
    );

    let focused = reconcile_runtime_arrival(
        &snapshot,
        RuntimeArrival::remote_tab(
            "window-1",
            "workspace-1",
            "pane-mixed",
            "surface-focused",
            "%43",
            1,
            "surface-terminal",
            true,
        ),
    );
    assert_eq!(
        focused.snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-focused")
    );
}

#[test]
fn dock_api_combined_invalid_inputs_follow_frozen_validation_and_owner_precedence() {
    let invalid_provider = main_transition(
        &main_window_snapshot(),
        "surface.create",
        json!({"placement":"dock","type":"agentSession","provider":"invalid"}),
        false,
        false,
    );
    assert_error(
        &invalid_provider,
        "invalid_params",
        "Invalid provider (codex|claude|opencode)",
    );

    let unsupported_before_disabled = main_transition(
        &main_window_snapshot(),
        "surface.create",
        json!({"placement":"dock","type":"markdown"}),
        false,
        false,
    );
    assert_error(
        &unsupported_before_disabled,
        "invalid_params",
        "Dock placement supports only terminal and browser surfaces",
    );

    let direction_before_placement = main_transition(
        &main_window_snapshot(),
        "pane.create",
        json!({"placement":"invalid","direction":"diagonal"}),
        true,
        true,
    );
    assert_error(
        &direction_before_placement,
        "invalid_params",
        "Missing or invalid direction (left|right|up|down)",
    );
    // Canonical: TerminalController+ControlPaneContext.swift:297-300 resolves
    // placement BEFORE the divider guard at :313-319 (the previous assertion
    // pinned the reversed, non-canonical order).
    let placement_before_divider = main_transition(
        &main_window_snapshot(),
        "pane.create",
        json!({"placement":"invalid","direction":"right","initial_divider_position":"wide"}),
        true,
        true,
    );
    assert_eq!(
        assert_error(
            &placement_before_divider,
            "invalid_params",
            "placement must be one of: workspace, dock",
        ),
        json!({"placement":"invalid"})
    );

    let invalid_url = main_transition(
        &main_window_snapshot(),
        "surface.create",
        json!({"placement":"dock","type":"browser","url":"http://["}),
        true,
        false,
    );
    assert_eq!(
        assert_error(&invalid_url, "invalid_params", "Invalid URL"),
        json!({"url":"http://["})
    );

    let mut snapshot = two_window_snapshot();
    snapshot.windows[0].window_id = Some("main".into());
    let created = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .unwrap();
    let conflict = main_transition(
        &snapshot,
        "surface.create",
        json!({"placement":"dock","window_id":"window-2","pane_id":created.pane_id.to_string()}),
        true,
        true,
    );
    assert_error(
        &conflict,
        "invalid_params",
        "Conflicting Dock routing selectors",
    );
    let unresolved = main_transition(
        &snapshot,
        "surface.create",
        json!({"placement":"dock","window_id":"missing-window"}),
        true,
        true,
    );
    assert_error(&unresolved, "unavailable", "TabManager not available");
}

#[test]
fn dock_api_pane_create_maps_direction_to_dock_split_without_touching_workspace_focus() {
    let (snapshot, source_pane_id, source_surface_id) = main_dock_snapshot();
    let workspace_before = snapshot.windows[0].tab_manager.clone();
    let created = main_transition(
        &snapshot,
        "pane.create",
        json!({
            "placement":"dock",
            "direction":"down",
            "surface_id":source_surface_id,
            "type":"browser",
            "url":"https://split.test",
            "initial_divider_position":0.4,
            "focus":false,
        }),
        true,
        true,
    );
    let value = ok_value(&created);
    assert_eq!(value["window_id"], "main");
    assert_eq!(value["workspace_id"], "main");
    assert_eq!(value["placement"], "dock");
    assert_eq!(value["pane_id"], Value::Null);
    assert_eq!(value["surface_id"], Value::Null);
    let dock_pane_id = value["dock_pane_id"].as_str().expect("new Dock pane");
    let dock_surface_id = value["dock_surface_id"].as_str().expect("new Dock surface");
    assert_ne!(dock_pane_id, source_pane_id);
    assert_ne!(dock_surface_id, source_surface_id);
    assert_eq!(created.snapshot.windows[0].tab_manager, workspace_before);
    assert!(!created
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::ActivateWindow { .. })));
    let dock = DockStore.snapshot(&created.snapshot, "main");
    assert_eq!(dock.panes.len(), 2);
    let pane = dock
        .panes
        .iter()
        .find(|pane| pane.id.to_string() == dock_pane_id)
        .unwrap();
    assert_eq!(pane.placement, "split_down");
    assert_eq!(pane.divider_position, Some(0.4));
    assert_eq!(
        pane.surface_ids,
        vec![Uuid::parse_str(dock_surface_id).unwrap()]
    );
    assert_eq!(
        created
            .events
            .iter()
            .filter(|event| event.name == "pane.created")
            .count(),
        1
    );
    assert_eq!(
        created
            .events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );
}

#[test]
fn dock_api_read_focus_and_close_route_through_the_main_owner() {
    let (mut snapshot, pane_id, first_surface_id) = main_dock_snapshot();
    let second = DockStore
        .create(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Browser,
                pane_id: Some(Uuid::parse_str(&pane_id).unwrap()),
                url: Some("https://read.test".into()),
                focus: false,
                ..DockCreateRequest::default()
            },
        )
        .unwrap();

    let listed = main_transition(
        &snapshot,
        "surface.list",
        json!({"workspace_id":"main"}),
        true,
        true,
    );
    let list_value = ok_value(&listed);
    assert_eq!(list_value["window_id"], "main");
    assert_eq!(list_value["workspace_id"], "main");
    let second_surface_id = second.surface_id.to_string();
    let listed_ids = list_value["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|surface| surface["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        listed_ids,
        vec![first_surface_id.as_str(), second_surface_id.as_str()]
    );
    assert!(!listed.changed);
    assert!(listed.effects.is_empty());

    let current = main_transition(
        &snapshot,
        "surface.current",
        json!({"workspace_id":"main"}),
        true,
        true,
    );
    let current_value = ok_value(&current);
    assert_eq!(current_value["window_id"], "main");
    assert_eq!(current_value["workspace_id"], "main");
    assert_eq!(current_value["surface_id"], first_surface_id);

    let focused = main_transition(
        &snapshot,
        "surface.focus",
        json!({"surface_id":second.surface_id}),
        true,
        true,
    );
    let focused_value = ok_value(&focused);
    assert_eq!(focused_value["window_id"], "main");
    assert_eq!(focused_value["workspace_id"], "main");
    assert_eq!(focused_value["surface_id"], second.surface_id.to_string());
    assert_eq!(
        DockStore
            .current(&focused.snapshot, "main")
            .unwrap()
            .surface_id,
        second.surface_id
    );
    assert!(focused.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::DockReveal { owner_id } if owner_id == "main"
    )));
    assert_eq!(
        focused
            .effects
            .iter()
            .filter(|effect| matches!(effect, LifecycleEffect::DockChanged { .. }))
            .count(),
        1
    );

    let closed = main_transition(
        &focused.snapshot,
        "surface.close",
        json!({"workspace_id":"main"}),
        true,
        true,
    );
    let closed_value = ok_value(&closed);
    assert_eq!(closed_value["window_id"], "main");
    assert_eq!(closed_value["workspace_id"], "main");
    assert_eq!(closed_value["surface_id"], second.surface_id.to_string());
    assert!(DockStore
        .list(&closed.snapshot, "main")
        .iter()
        .all(|surface| surface.surface_id != second.surface_id));
    assert_eq!(
        closed
            .effects
            .iter()
            .filter(|effect| matches!(effect, LifecycleEffect::DockChanged { .. }))
            .count(),
        1
    );
    let closed_event = closed
        .events
        .iter()
        .find(|event| event.name == "surface.closed")
        .unwrap();
    assert_eq!(closed_event.window_id.as_deref(), Some("main"));
    assert_eq!(closed_event.workspace_id.as_deref(), Some("main"));
    assert_eq!(closed_event.pane_id.as_deref(), Some(pane_id.as_str()));
    assert_eq!(
        closed_event.surface_id.as_deref(),
        Some(second_surface_id.as_str())
    );
}

#[test]
fn dock_api_move_preserves_generation_persistence_and_one_owner_across_containers() {
    let (snapshot, dock_pane_id, dock_surface_id) = main_dock_snapshot();
    let workspace_created = main_transition(
        &snapshot,
        "surface.create",
        json!({"pane_id":"pane-1", "type":"terminal", "focus":false}),
        true,
        true,
    );
    let workspace_id = ok_value(&workspace_created)["surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = SurfaceLifecycleModel::from_app_session(&workspace_created.snapshot).unwrap();
    let generation = before.surface(&workspace_id).unwrap().generation;

    let moved_into_dock = main_transition(
        &workspace_created.snapshot,
        "surface.move",
        json!({
            "surface_id":workspace_id,
            "pane_id":dock_pane_id,
            "index":0,
            "focus":false,
        }),
        true,
        true,
    );
    let moved_value = ok_value(&moved_into_dock);
    assert_eq!(moved_value["window_id"], "main");
    assert_eq!(moved_value["workspace_id"], "main");
    assert_eq!(moved_value["pane_id"], dock_pane_id);
    let dock_model = SurfaceLifecycleModel::from_app_session(&moved_into_dock.snapshot).unwrap();
    dock_model.validate_indexes().unwrap();
    assert_eq!(
        dock_model.surface(&workspace_id).unwrap().generation,
        generation
    );
    assert_eq!(
        dock_model
            .pane(&dock_model.owner_of_surface(&workspace_id).unwrap().pane_id)
            .unwrap()
            .container,
        ContainerKind::Dock
    );
    assert_eq!(
        dock_model
            .snapshot()
            .panes
            .iter()
            .flat_map(|pane| pane.surface_ids.iter())
            .filter(|surface_id| *surface_id == &workspace_id)
            .count(),
        1
    );

    let moved_out = main_transition(
        &moved_into_dock.snapshot,
        "surface.move",
        json!({
            "surface_id":dock_surface_id,
            "pane_id":"pane-1",
            "index":0,
            "focus":false,
        }),
        true,
        true,
    );
    let moved_out_value = ok_value(&moved_out);
    assert_eq!(moved_out_value["workspace_id"], "workspace-1");
    assert_eq!(moved_out_value["pane_id"], "pane-1");
    let restored = decode_session(&encode_session(&moved_out.snapshot).unwrap()).unwrap();
    let restored_model = SurfaceLifecycleModel::from_app_session(&restored).unwrap();
    restored_model.validate_indexes().unwrap();
    assert_eq!(
        restored_model
            .snapshot()
            .panes
            .iter()
            .flat_map(|pane| pane.surface_ids.iter())
            .filter(|surface_id| *surface_id == &dock_surface_id)
            .count(),
        1
    );
    assert_eq!(
        restored_model
            .pane(
                &restored_model
                    .owner_of_surface(&dock_surface_id)
                    .unwrap()
                    .pane_id
            )
            .unwrap()
            .container,
        ContainerKind::Workspace
    );
}

#[test]
fn dock_api_stage_failure_rolls_back_without_publishing_claimed_identity() {
    let mut snapshot = main_window_snapshot();
    let before = snapshot.clone();
    let planned = main_transition(
        &snapshot,
        "surface.create",
        json!({"placement":"dock", "type":"terminal", "focus":false}),
        true,
        true,
    );
    let claimed_id = ok_value(&planned)["dock_surface_id"]
        .as_str()
        .expect("planned Dock identity")
        .to_owned();
    let mut executor = RecordingExecutor {
        fail_stage_at: Some(0),
        ..RecordingExecutor::default()
    };
    assert!(commit_lifecycle_transition(&mut snapshot, planned, &mut executor).is_err());
    assert_eq!(snapshot, before);
    assert_eq!(executor.rollback_count, 1);
    assert!(executor.staged.is_empty());
    assert!(executor.committed.is_empty());
    assert!(SurfaceLifecycleModel::from_app_session(&snapshot)
        .unwrap()
        .surface(&claimed_id)
        .is_none());
    assert!(DockStore.list(&snapshot, "main").is_empty());
}

#[test]
fn dock_api_runtime_stage_receives_reserved_identity_generation_and_intent() {
    let mut snapshot = main_window_snapshot();
    let reserved = Uuid::new_v4();
    let created = DockStore
        .create_transactionally(
            &mut snapshot,
            "main",
            DockCreateRequest {
                kind: DockSurfaceKind::Browser,
                url: Some("https://runtime.test".into()),
                browser_profile: Some("isolated".into()),
                surface_id: Some(reserved),
                focus: false,
                ..DockCreateRequest::default()
            },
            |operation| {
                assert_eq!(
                    operation,
                    &DockRuntimeOperation::Create {
                        surface_id: reserved.to_string(),
                        generation: 1,
                        intent: DockRuntimeIntent::Browser {
                            url: "https://runtime.test".into(),
                            profile: Some("isolated".into()),
                        },
                    }
                );
                Ok::<(), String>(())
            },
        )
        .unwrap();
    assert_eq!(created.surface_id, reserved);
    assert_eq!(created.generation, 1);
    let model = SurfaceLifecycleModel::from_app_session(&snapshot).unwrap();
    assert_eq!(model.surface(&reserved.to_string()).unwrap().generation, 1);
    assert_eq!(
        model
            .pane(
                &model
                    .owner_of_surface(&reserved.to_string())
                    .unwrap()
                    .pane_id
            )
            .unwrap()
            .container,
        ContainerKind::Dock
    );
}

fn pane_created_source_pane(transition: &LifecycleTransition) -> String {
    transition
        .events
        .iter()
        .find(|event| event.name == "pane.created")
        .expect("pane.created event")
        .payload["source_pane_id"]
        .as_str()
        .expect("source_pane_id")
        .to_owned()
}

#[test]
fn pane_create_rejects_agent_session_before_provider_placement_and_workspace() {
    // Canonical: ControlCommandCoordinator+Pane.swift:330-331 +
    // TerminalController+ControlPaneContext.swift:293-296 — pane.create rejects
    // type=agent-session right after the direction parse, before provider
    // validation, placement parsing, and workspace resolution; the error data
    // echoes PanelType.agentSession.rawValue ("agentSession").
    for method in ["pane.create", "surface.split"] {
        // surface.split shares the reject verbatim:
        // ControlCommandCoordinator+Surface.swift:330-334.
        let rejected = transition(
            &test_snapshot(),
            method,
            json!({
                "direction": "right",
                "type": "agent-session",
                "provider": "bogus",
                "placement": "bogus",
                "workspace_id": "missing-workspace"
            }),
        );
        assert_eq!(
            assert_error(
                &rejected,
                "invalid_params",
                "agent-session is only supported by surface.create"
            ),
            json!({"type": "agentSession"}),
            "{method}"
        );
        assert!(!rejected.changed);
    }
}

#[test]
fn pane_create_checks_tab_manager_availability_before_direction() {
    // Canonical: ControlCommandCoordinator+Pane.swift:287-291 — the routing
    // TabManager guard runs before any input validation, so an unresolvable
    // explicit window_id errors unavailable even when direction is missing.
    let unavailable = transition(
        &test_snapshot(),
        "pane.create",
        json!({"window_id": "missing-window"}),
    );
    assert_error(&unavailable, "unavailable", "TabManager not available");
}

#[test]
fn pane_create_validates_placement_before_divider_position() {
    // Canonical: TerminalController+ControlPaneContext.swift:297-300 resolves
    // placement before the divider guard at :313-319, so an invalid placement
    // wins over a non-numeric initial_divider_position.
    let invalid = transition(
        &test_snapshot(),
        "pane.create",
        json!({
            "direction": "right",
            "placement": "bogus",
            "initial_divider_position": {"nested": true}
        }),
    );
    assert_eq!(
        assert_error(
            &invalid,
            "invalid_params",
            "placement must be one of: workspace, dock"
        ),
        json!({"placement": "bogus"})
    );
}

#[test]
fn pane_create_browser_disabled_follows_canonical_outcomes_and_order() {
    // Canonical: TerminalController+ControlPaneContext.swift:308-310 + :437-452
    // — with the browser disabled, pane.create resolves the browser-disabled
    // outcome before divider validation and before workspace resolution.
    let no_url = main_transition(
        &test_snapshot(),
        "pane.create",
        json!({"direction": "right", "type": "browser"}),
        true,
        false,
    );
    assert_error(&no_url, "browser_disabled", "cmux browser is disabled");

    let invalid_url = main_transition(
        &test_snapshot(),
        "pane.create",
        json!({"direction": "right", "type": "browser", "url": "http://["}),
        true,
        false,
    );
    assert_eq!(
        assert_error(&invalid_url, "invalid_params", "Invalid URL"),
        json!({"url": "http://["})
    );

    let before_divider = main_transition(
        &test_snapshot(),
        "pane.create",
        json!({
            "direction": "right",
            "type": "browser",
            "initial_divider_position": [1],
            "workspace_id": "missing-workspace"
        }),
        true,
        false,
    );
    assert_error(&before_divider, "browser_disabled", "cmux browser is disabled");

    let external = main_transition(
        &test_snapshot(),
        "pane.create",
        json!({"direction": "right", "type": "browser", "url": "https://ext.test/x"}),
        true,
        false,
    );
    let value = ok_value(&external);
    assert_eq!(
        value,
        json!({
            "window_id": "window-1",
            "workspace_id": Value::Null,
            "pane_id": Value::Null,
            "surface_id": Value::Null,
            "created_split": false,
            "opened_externally": true,
            "browser_disabled": true,
            "placement_strategy": "external_browser_disabled",
            "url": "https://ext.test/x"
        })
    );
    assert!(external.events.is_empty());
    assert!(external.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::ExternalBrowserOpen { url, .. } if url == "https://ext.test/x"
    )));
}

#[test]
fn placement_parsing_trims_lowercases_and_maps_canonical_aliases() {
    // Canonical: TerminalController+ControlPaneContext.swift:788-802
    // (resolveControlPlacement) — trim + lowercase; empty/main/content/split →
    // workspace; rightsidebardock/right-sidebar-dock/sidebar → dock; anything
    // else invalid with the ORIGINAL raw value echoed.
    for placement in ["main", " Content ", "SPLIT", "", "workspace", " Main\n"] {
        let created = transition(
            &test_snapshot(),
            "pane.create",
            json!({"direction": "right", "type": "terminal", "placement": placement}),
        );
        let value = ok_value(&created);
        assert!(value["pane_id"].is_string(), "{placement:?}");
        assert!(value.get("placement").is_none(), "{placement:?}");
    }
    for placement in ["Sidebar", "right-sidebar-dock", "RIGHTSIDEBARDOCK", " dock \n"] {
        let created = main_transition(
            &main_window_snapshot(),
            "pane.create",
            json!({"direction": "right", "type": "terminal", "placement": placement}),
            true,
            true,
        );
        let value = ok_value(&created);
        assert_eq!(value["placement"], json!("dock"), "{placement:?}");
    }
    let invalid = transition(
        &test_snapshot(),
        "surface.create",
        json!({"type": "terminal", "placement": " Bogus "}),
    );
    assert_eq!(
        assert_error(
            &invalid,
            "invalid_params",
            "placement must be one of: workspace, dock"
        ),
        json!({"placement": " Bogus "})
    );
}

#[test]
fn pane_create_source_is_raw_uuid_only_with_focused_fallback() {
    // Canonical: ControlCommandCoordinator+Pane.swift:300 — the split source
    // comes ONLY from surface_id parsed as a UUID; any other string (including
    // surface:N refs) falls back to the workspace focused surface
    // (TerminalController+ControlPaneContext.swift:342-345). Windows fixtures
    // use non-UUID surface ids, so "parses as a UUID" adapts to "is a UUID or
    // an existing surface id"; minted kind:N refs never collide with either.
    let snapshot = resizable_snapshot();
    let ref_shaped = transition(
        &snapshot,
        "pane.create",
        json!({"direction": "right", "surface_id": "surface:9"}),
    );
    assert!(ok_value(&ref_shaped)["surface_id"].is_string());
    assert_eq!(pane_created_source_pane(&ref_shaped), "pane-left");

    let existing_id = transition(
        &snapshot,
        "pane.create",
        json!({"direction": "right", "surface_id": "surface-right"}),
    );
    assert_eq!(pane_created_source_pane(&existing_id), "pane-right");

    // A syntactically valid UUID that is not in the workspace is terminal:
    // canonical guards ws.panels[sourcePanelId] without falling back.
    let unknown_uuid = transition(
        &snapshot,
        "pane.create",
        json!({
            "direction": "right",
            "surface_id": "123e4567-e89b-42d3-a456-426614174000"
        }),
    );
    assert_error(&unknown_uuid, "not_found", "No source surface to split");

    // No focused surface and no source: canonical returns noSourceSurface —
    // there is no first-layout-surface fallback.
    let mut unfocused = resizable_snapshot();
    unfocused.windows[0].tab_manager.workspaces[0].focused_panel_id = None;
    let no_source = transition(&unfocused, "pane.create", json!({"direction": "right"}));
    assert_error(&no_source, "not_found", "No source surface to split");
}

#[test]
fn pane_create_applies_and_persists_full_terminal_startup_metadata() {
    // Canonical: TerminalController+ControlPaneContext.swift:374-384 —
    // pane.create forwards trimmed initial_command / working_directory /
    // tmux_start_command and the startup_environment (startup_environment then
    // initial_env, trimmed non-empty keys) to newTerminalSplitOutcome, and the
    // created surface persists that startup metadata.
    let created = transition(
        &test_snapshot(),
        "pane.create",
        json!({
            "direction": "right",
            "type": "terminal",
            "initial_command": "  cargo run  ",
            "working_directory": " C:/w ",
            "tmux_start_command": " htop ",
            "startup_environment": {"FOO": "bar", "  ": "dropped"}
        }),
    );
    let value = ok_value(&created);
    let created_id = value["surface_id"].as_str().unwrap();
    let effect = created
        .effects
        .iter()
        .find(|effect| matches!(effect, LifecycleEffect::TerminalCreate { .. }))
        .expect("terminal create effect");
    let effect = serde_json::to_value(effect).unwrap();
    assert_eq!(effect["TerminalCreate"]["command"], json!("cargo run"));
    assert_eq!(effect["TerminalCreate"]["working_directory"], json!("C:/w"));
    assert_eq!(effect["TerminalCreate"]["tmux_start_command"], json!("htop"));
    assert_eq!(
        effect["TerminalCreate"]["startup_environment"],
        json!({"FOO": "bar"})
    );
    let record = created.snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|record| record.surface_id == created_id)
        .expect("created surface record")
        .clone();
    assert_eq!(
        record.terminal_startup,
        Some(cmux_core::session::SessionSurfaceTerminalStartupSnapshot {
            command: Some("cargo run".into()),
            working_directory: Some("C:/w".into()),
            tmux_start_command: Some("htop".into()),
            environment: Some(std::collections::BTreeMap::from([(
                "FOO".to_string(),
                "bar".to_string()
            )])),
            ..Default::default()
        })
    );
}
