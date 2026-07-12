use super::pane_surface_lifecycle::{
    commit_lifecycle_transition, dispatch_lifecycle_request, reconcile_runtime_arrival,
    LifecycleDispatchContext, LifecycleEffect, LifecycleEffectExecutor, LifecycleTransition,
    RuntimeArrival,
};
use super::*;
use cmux_core::surface_lifecycle::{AttachOutcome, RuntimeHandle, SurfaceLifecycleModel};

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

#[derive(Default)]
struct RecordingExecutor {
    staged: Vec<LifecycleEffect>,
    committed: Vec<LifecycleEffect>,
    fail_stage_at: Option<usize>,
    rollback_count: usize,
}

impl LifecycleEffectExecutor for RecordingExecutor {
    type Error = &'static str;

    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error> {
        if self.fail_stage_at == Some(self.staged.len()) {
            return Err("injected effect failure");
        }
        self.staged.push(effect.clone());
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        self.committed.append(&mut self.staged);
        Ok(())
    }

    fn rollback_staged(&mut self) {
        self.rollback_count += 1;
        self.staged.clear();
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

    let remote_snapshot = mark_remote_tmux_workspace(test_snapshot(), 0);
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
    assert_error(
        &invalid,
        "invalid_params",
        "Missing or invalid surface_id",
    );
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
