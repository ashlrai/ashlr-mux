use super::*;

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
    snapshot.windows[0].selected_workspace_id = Some("workspace-1".to_string());
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-1".to_string());

    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("window-2".to_string());
    second.selected_workspace_id = Some("workspace-2".to_string());
    second.tab_manager.selected_workspace_index = Some(0);
    second.tab_manager.workspaces[0].workspace_id = Some("workspace-2".to_string());
    second.tab_manager.workspaces[0].focused_panel_id = Some("surface-2".to_string());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = second.tab_manager.workspaces[0]
        .layout
        .as_mut()
        .expect("second window layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2".to_string());
    pane.panel_ids = vec!["surface-2".to_string()];
    pane.selected_panel_id = Some("surface-2".to_string());
    snapshot.windows.push(second);
    snapshot
}

fn mixed_surface_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-terminal".to_string());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("mixed surface layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-mixed".to_string());
    pane.panel_ids = vec![
        "surface-terminal".to_string(),
        "surface-browser".to_string(),
    ];
    pane.selected_panel_id = Some("surface-browser".to_string());
    // This is deliberately the legacy pane-wide approximation. The RED
    // assertions below require the decoded snapshot projection to stop
    // leaking this kind across both public surfaces.
    pane.surface_kind = Some("browser".to_string());
    pane.browser_url = Some("https://example.test".to_string());
    snapshot
}

fn resizable_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-left".to_string());
    let SessionWorkspaceLayoutSnapshot::Pane(base) = workspace.layout.take().expect("base pane")
    else {
        unreachable!();
    };
    let mut left = base.clone();
    left.pane_id = Some("pane-left".to_string());
    left.panel_ids = vec!["surface-left".to_string()];
    left.selected_panel_id = Some("surface-left".to_string());
    let mut right = base;
    right.pane_id = Some("pane-right".to_string());
    right.panel_ids = vec!["surface-right".to_string()];
    right.selected_panel_id = Some("surface-right".to_string());
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Split(
        cmux_core::session::SessionSplitLayoutSnapshot {
            split_id: Some("split-root".to_string()),
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(left)),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(right)),
        },
    ));
    snapshot
}

fn ok_value(result: ControlCallResult) -> Value {
    let ControlCallResult::Ok(value) = result else {
        panic!("expected successful decoded request result")
    };
    value.into()
}

// This is the intended tests-only decoded request seam. It must execute the
// same pure routing/validation/model mutation/event path as the desktop
// handler without constructing a Tauri runtime. The third value records
// observable non-snapshot effects (focus/activation, runtime replacement,
// Dock/remote routing, and persistence writes).
fn decoded_lifecycle_call(
    snapshot: &mut AppSessionSnapshot,
    method: &str,
    params: Value,
) -> (ControlCallResult, Vec<DerivedEventSpec>, Value) {
    pane_surface_lifecycle_request_for_test(
        snapshot,
        method,
        params.as_object().expect("decoded params object"),
    )
}

fn decoded_lifecycle_ok(
    snapshot: &mut AppSessionSnapshot,
    method: &str,
    params: Value,
) -> (Value, Vec<DerivedEventSpec>, Value) {
    let (result, events, effects) = decoded_lifecycle_call(snapshot, method, params);
    (ok_value(result), events, effects)
}

fn assert_error(result: ControlCallResult, expected_code: &str, expected_message: &str) -> Value {
    let ControlCallResult::Err {
        code,
        message,
        data,
    } = result
    else {
        panic!("expected decoded request error")
    };
    assert_eq!(code, expected_code);
    assert_eq!(message, expected_message);
    data.map(Value::from).unwrap_or(Value::Null)
}

#[test]
fn v2_lifecycle_methods_are_all_advertised_as_capabilities() {
    let advertised: BTreeSet<_> = CONTROL_SOCKET_METHODS.iter().copied().collect();
    let missing: Vec<_> = V2_LIFECYCLE_METHODS
        .into_iter()
        .filter(|method| !advertised.contains(method))
        .collect();
    assert!(missing.is_empty(), "missing lifecycle methods: {missing:?}");
}

#[test]
fn v2_pane_create_capability_is_distinct_from_surface_split() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.split"));
    assert!(CONTROL_SOCKET_METHODS.contains(&"pane.create"));

    let mut snapshot = test_snapshot();
    let (invalid, invalid_events, invalid_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "pane.create",
        json!({"direction": "diagonal", "placement": "somewhere"}),
    );
    assert_error(
        invalid,
        "invalid_params",
        "Missing or invalid direction (left|right|up|down)",
    );
    assert!(invalid_events.is_empty());
    assert_eq!(invalid_effects["persistence_write_count"], json!(0));

    let before_focus = snapshot.windows[0].tab_manager.workspaces[0]
        .focused_panel_id
        .clone();
    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "pane.create",
        json!({
            "direction": "right",
            "type": "browser",
            "url": "https://example.test",
            "initial_divider_position": 0.75,
            "focus": false,
        }),
    );
    assert_eq!(value["type"], json!("browser"));
    assert!(value["pane_id"].is_string());
    assert!(value["surface_id"].is_string());
    assert_ne!(value["pane_id"], json!("pane-1"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "pane.created")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );
    assert_eq!(effects["focus_changed"], json!(false));
    assert_eq!(effects["window_activation_count"], json!(0));
    assert_eq!(effects["persistence_write_count"], json!(1));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id,
        before_focus
    );
}

#[test]
fn v2_pane_resize_absolute_intent_has_validation_precedence() {
    let mut snapshot = test_snapshot();
    let before = snapshot.clone();
    let (result, events, effects) = decoded_lifecycle_call(
        &mut snapshot,
        "pane.resize",
        json!({
            "absolute_axis": "diagonal",
            "target_pixels": 640,
            "direction": "left",
            "amount": 2,
        }),
    );
    assert_error(
        result,
        "invalid_params",
        "absolute_axis must be 'horizontal' or 'vertical'",
    );
    assert!(events.is_empty());
    assert_eq!(effects["persistence_write_count"], json!(0));
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::to_value(before).unwrap()
    );

    let mut snapshot = resizable_snapshot();
    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "pane.resize",
        json!({"pane_id": "pane-left", "direction": "right", "amount": 2}),
    );
    assert_eq!(value["pane_id"], json!("pane-left"));
    assert_eq!(value["split_id"], json!("split-root"));
    assert_eq!(value["old_divider_position"], json!(0.5));
    assert!(value["new_divider_position"].as_f64().unwrap() > 0.5);
    assert_eq!(value["direction"], json!("right"));
    assert_eq!(value["amount"], json!(2));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "pane.resized")
            .count(),
        1
    );
    assert_eq!(effects["focus_changed"], json!(false));
    assert_eq!(effects["window_activation_count"], json!(0));
    assert_eq!(effects["persistence_write_count"], json!(1));
    let SessionWorkspaceLayoutSnapshot::Split(split) = snapshot.windows[0].tab_manager.workspaces
        [0]
    .layout
    .as_ref()
    .unwrap() else {
        unreachable!();
    };
    assert_eq!(
        split.divider_position,
        value["new_divider_position"].as_f64().unwrap()
    );
}

#[test]
fn v2_surface_action_and_create_are_not_approximated_by_legacy_helpers() {
    for method in ["surface.action", "surface.create"] {
        assert!(
            CONTROL_SOCKET_METHODS.contains(&method),
            "{method} must be an exact v2 decoded request route"
        );
    }
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.new_terminal_tab"));

    let mut snapshot = test_snapshot();
    let (invalid_action, invalid_action_events, invalid_action_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.action",
        json!({"surface_id": "surface-1"}),
    );
    assert_error(invalid_action, "invalid_params", "Missing action");
    assert!(invalid_action_events.is_empty());
    assert_eq!(invalid_action_effects["persistence_write_count"], json!(0));

    let (invalid_create, invalid_create_events, invalid_create_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.create",
        json!({"pane_id": "pane-1", "type": "agentSession", "provider": "bogus"}),
    );
    let provider_data = assert_error(
        invalid_create,
        "invalid_params",
        "Invalid provider (codex|claude|opencode)",
    );
    assert_eq!(provider_data, json!({"provider": "bogus"}));
    assert!(invalid_create_events.is_empty());
    assert_eq!(invalid_create_effects["persistence_write_count"], json!(0));

    let (created, create_events, create_effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.create",
        json!({
            "pane_id": "pane-1",
            "type": "terminal",
            "working_directory": "C:/created",
            "initial_command": "cargo test",
            "focus": false,
        }),
    );
    assert_eq!(created["pane_id"], json!("pane-1"));
    assert_eq!(created["type"], json!("terminal"));
    assert!(created["surface_id"].is_string());
    assert_eq!(
        create_events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );
    assert_eq!(create_effects["persistence_write_count"], json!(1));
    assert_eq!(create_effects["window_activation_count"], json!(0));

    let created_id = created["surface_id"].as_str().unwrap();
    assert!(
        surfaces_for_workspace(&snapshot.windows[0].tab_manager.workspaces[0])
            .iter()
            .any(|surface| surface["id"] == json!(created_id))
    );

    let (renamed, action_events, action_effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.action",
        json!({"surface_id": created_id, "action": "rename", "title": " Build logs "}),
    );
    assert_eq!(renamed["action"], json!("rename"));
    assert_eq!(renamed["surface_id"], json!(created_id));
    assert_eq!(renamed["title"], json!("Build logs"));
    assert_eq!(
        action_events
            .iter()
            .filter(|event| event.name == "surface.action")
            .count(),
        1
    );
    assert_eq!(action_effects["persistence_write_count"], json!(1));
    assert_eq!(action_effects["window_activation_count"], json!(0));
}

#[test]
fn v2_pane_and_surface_create_expose_dock_and_remote_routing_contracts() {
    for method in ["pane.create", "surface.create"] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
    }
    let mut dock_snapshot = test_snapshot();
    let (dock, dock_events, dock_effects) = decoded_lifecycle_ok(
        &mut dock_snapshot,
        "surface.create",
        json!({
            "placement": "dock",
            "type": "browser",
            "url": "https://dock.test",
            "focus": false,
        }),
    );
    assert_eq!(dock["placement"], json!("dock"));
    assert!(dock["pane_id"].is_null());
    assert!(dock["surface_id"].is_null());
    assert!(dock["dock_surface_id"].is_string());
    assert_eq!(dock_effects["dock_surface_count"], json!(1));
    assert_eq!(dock_effects["window_activation_count"], json!(0));
    assert_eq!(dock_effects["persistence_write_count"], json!(1));
    assert_eq!(
        dock_events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );

    let mut remote_snapshot = test_snapshot();
    let (remote, remote_events, remote_effects) = decoded_lifecycle_ok(
        &mut remote_snapshot,
        "pane.create",
        json!({
            "direction": "right",
            "remote_pty_session_id": "remote-session-1",
            "remote_context": {"transport": "tmux"},
        }),
    );
    assert_eq!(remote["accepted"], json!(true));
    assert_eq!(remote["routed"], json!("remote-tmux"));
    assert!(remote["pane_id"].is_null());
    assert!(remote["surface_id"].is_null());
    assert!(remote_events.is_empty(), "arrival owns lifecycle events");
    assert_eq!(remote_effects["remote_request_count"], json!(1));
    assert_eq!(remote_effects["persistence_write_count"], json!(0));
}

#[test]
fn v2_surface_current_is_a_read_capability_without_focus_side_effects() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.current"));
    let mut snapshot = mixed_surface_snapshot();
    let before = serde_json::to_value(&snapshot).unwrap();
    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.current",
        json!({"surface_id": "surface-browser"}),
    );
    assert_eq!(value["window_id"], json!("window-1"));
    assert_eq!(value["workspace_id"], json!("workspace-1"));
    assert_eq!(value["pane_id"], json!("pane-mixed"));
    assert_eq!(value["surface_id"], json!("surface-terminal"));
    assert_eq!(value["surface_type"], json!("terminal"));
    assert!(events.is_empty());
    assert_eq!(effects["window_activation_count"], json!(0));
    assert_eq!(effects["persistence_write_count"], json!(0));
    assert_eq!(serde_json::to_value(snapshot).unwrap(), before);
}

#[test]
fn v2_surface_list_routes_to_the_explicit_second_window_without_fallback() {
    let mut snapshot = two_window_snapshot();
    let before = serde_json::to_value(&snapshot).unwrap();
    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.list",
        json!({"window_id": "window-2"}),
    );
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["window_ref"], json!("window:2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["surfaces"][0]["id"], json!("surface-2"));
    assert!(events.is_empty());
    assert_eq!(effects["window_activation_count"], json!(0));
    assert_eq!(effects["persistence_write_count"], json!(0));
    assert_eq!(serde_json::to_value(snapshot).unwrap(), before);
}

#[test]
fn v2_surface_list_rejects_an_invalid_explicit_window_before_workspace_fallback() {
    let mut snapshot = two_window_snapshot();
    let before = serde_json::to_value(&snapshot).unwrap();
    let (result, events, effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.list",
        json!({
            "window_id": "missing-window",
            "workspace_id": "workspace-2",
        }),
    );
    assert_error(result, "unavailable", "TabManager not available");
    assert!(events.is_empty());
    assert_eq!(effects["persistence_write_count"], json!(0));
    assert_eq!(serde_json::to_value(snapshot).unwrap(), before);
}

#[test]
fn v2_surface_list_projects_mixed_kinds_and_distinct_selection_and_focus() {
    let value = ok_value(surface_list(&mixed_surface_snapshot()));
    let surfaces = value["surfaces"].as_array().expect("surfaces array");
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0]["id"], json!("surface-terminal"));
    assert_eq!(surfaces[0]["index"], json!(0));
    assert_eq!(surfaces[0]["index_in_pane"], json!(0));
    assert_eq!(surfaces[0]["type"], json!("terminal"));
    assert_eq!(surfaces[0]["focused"], json!(true));
    assert_eq!(surfaces[0]["selected_in_pane"], json!(false));
    assert_eq!(surfaces[1]["id"], json!("surface-browser"));
    assert_eq!(surfaces[1]["index"], json!(1));
    assert_eq!(surfaces[1]["index_in_pane"], json!(1));
    assert_eq!(surfaces[1]["type"], json!("browser"));
    assert_eq!(surfaces[1]["focused"], json!(false));
    assert_eq!(surfaces[1]["selected_in_pane"], json!(true));
}

#[test]
fn v2_surface_list_uses_exact_kind_conditional_payload_fields() {
    let value = ok_value(surface_list(&mixed_surface_snapshot()));
    let surfaces = value["surfaces"].as_array().unwrap();
    assert!(surfaces[0].get("requested_working_directory").is_some());
    assert!(surfaces[0].get("resume_binding").is_some());
    assert!(surfaces[0].get("developer_tools_visible").is_none());
    assert!(surfaces[1].get("developer_tools_visible").is_some());
    assert!(surfaces[1].get("requested_working_directory").is_none());
    assert!(surfaces[1].get("initial_command").is_none());
}

#[test]
fn v2_surface_report_pwd_preserves_raw_path_and_detects_conflicting_aliases() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.report_pwd"));
    let mut snapshot = test_snapshot();
    let (conflict, conflict_events, conflict_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.report_pwd",
        json!({
            "workspace_id": "workspace-1",
            "path": "C:/one",
            "directory": "C:/two",
        }),
    );
    assert_error(conflict, "invalid_params", "Conflicting path parameters");
    assert!(conflict_events.is_empty());
    assert_eq!(conflict_effects["persistence_write_count"], json!(0));

    let (recorded, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.report_pwd",
        json!({
            "workspace_id": "workspace-1",
            "surface_id": "surface-1",
            "path": " C:/repo ",
            "cwd": " C:/repo ",
        }),
    );
    assert_eq!(recorded["workspace_id"], json!("workspace-1"));
    assert_eq!(recorded["surface_id"], json!("surface-1"));
    assert_eq!(recorded["path"], json!(" C:/repo "));
    assert!(events.is_empty());
    assert_eq!(effects["reported_directory"], json!(" C:/repo "));
    assert_eq!(effects["window_activation_count"], json!(0));
    assert_eq!(effects["persistence_write_count"], json!(1));
}

#[test]
fn v2_surface_respawn_is_an_exact_identity_preserving_capability() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.respawn"));
    let mut snapshot = test_snapshot();
    let (invalid, invalid_events, invalid_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.respawn",
        json!({"surface_id": "surface-1", "focus": "maybe"}),
    );
    assert_error(invalid, "invalid_params", "Missing or invalid focus");
    assert!(invalid_events.is_empty());
    assert_eq!(invalid_effects["runtime_replacement_count"], json!(0));

    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.respawn",
        json!({
            "surface_id": "surface-1",
            "command": "  cargo test  ",
            "initial_command": "ignored",
            "working_directory": "C:/respawn",
            "focus": false,
        }),
    );
    assert_eq!(value["surface_id"], json!("surface-1"));
    assert_eq!(value["type"], json!("terminal"));
    assert_eq!(effects["runtime_replacement_count"], json!(1));
    assert_eq!(effects["previous_runtime_generation"], json!(0));
    assert_eq!(effects["runtime_generation"], json!(1));
    assert_eq!(effects["initial_command"], json!("cargo test"));
    assert_eq!(effects["working_directory"], json!("C:/respawn"));
    assert_eq!(effects["focus_changed"], json!(false));
    assert_eq!(effects["persistence_write_count"], json!(1));
    assert!(!events
        .iter()
        .any(|event| matches!(event.name, "surface.created" | "surface.closed")));
}

#[test]
fn v2_surface_close_focus_and_move_keep_exact_public_routes() {
    for method in ["surface.close", "surface.focus", "surface.move"] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
    }
    let mut snapshot = mixed_surface_snapshot();
    let (closed, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.close",
        json!({"surface_id": "surface-browser"}),
    );
    assert_eq!(closed["surface_id"], json!("surface-browser"));
    let remaining = surfaces_for_workspace(&snapshot.windows[0].tab_manager.workspaces[0]);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["id"], json!("surface-terminal"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.closed")
            .count(),
        1
    );
    assert_eq!(effects["runtime_teardown_count"], json!(1));
    assert_eq!(effects["persistence_write_count"], json!(1));

    let (last, last_events, last_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.close",
        json!({"surface_id": "surface-terminal"}),
    );
    assert_error(last, "invalid_state", "Cannot close the last surface");
    assert!(last_events.is_empty());
    assert_eq!(last_effects["runtime_teardown_count"], json!(0));
    assert_eq!(last_effects["persistence_write_count"], json!(0));
}

#[test]
fn v2_surface_focus_uses_the_explicit_second_window_owner() {
    let mut snapshot = two_window_snapshot();
    snapshot.windows[1].tab_manager.workspaces[0].focused_panel_id = None;
    let (missing, missing_events, missing_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.focus",
        json!({"window_id": "window-2", "workspace_id": "workspace-2"}),
    );
    assert_error(missing, "invalid_params", "Missing or invalid surface_id");
    assert!(missing_events.is_empty());
    assert_eq!(missing_effects["persistence_write_count"], json!(0));

    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.focus",
        json!({
            "window_id": "window-2",
            "workspace_id": "workspace-2",
            "surface_id": "surface-2",
        }),
    );
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["surface_id"], json!("surface-2"));
    assert_eq!(
        snapshot.windows[1].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-2")
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.focused")
            .count(),
        1
    );
    assert_eq!(effects["window_activation_count"], json!(1));
    assert_eq!(effects["persistence_write_count"], json!(1));
}

#[test]
fn v2_surface_move_resolves_source_and_destination_across_windows() {
    let mut snapshot = two_window_snapshot();
    let (conflict, conflict_events, conflict_effects) = decoded_lifecycle_call(
        &mut snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-1",
            "before_surface_id": "surface-2",
            "after_surface_id": "surface-2",
        }),
    );
    assert_error(
        conflict,
        "invalid_params",
        "Specify at most one of before_surface_id or after_surface_id",
    );
    assert!(conflict_events.is_empty());
    assert_eq!(conflict_effects["persistence_write_count"], json!(0));

    let (value, events, effects) = decoded_lifecycle_ok(
        &mut snapshot,
        "surface.move",
        json!({
            "surface_id": "surface-1",
            "window_id": "window-2",
            "focus": true,
        }),
    );
    assert_eq!(value["surface_id"], json!("surface-1"));
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["pane_id"], json!("pane-2"));
    assert!(
        !surfaces_for_workspace(&snapshot.windows[0].tab_manager.workspaces[0])
            .iter()
            .any(|surface| surface["id"] == json!("surface-1"))
    );
    assert!(
        surfaces_for_workspace(&snapshot.windows[1].tab_manager.workspaces[0])
            .iter()
            .any(|surface| surface["id"] == json!("surface-1"))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.moved")
            .count(),
        1
    );
    assert_eq!(effects["window_activation_count"], json!(1));
    assert_eq!(effects["metadata_transfer_count"], json!(1));
    assert_eq!(effects["persistence_write_count"], json!(2));
}

#[test]
fn v2_lifecycle_snapshot_persists_per_surface_records_not_pane_wide_kind() {
    let encoded = serde_json::to_value(mixed_surface_snapshot()).expect("encode snapshot");
    let pane = &encoded["windows"][0]["tab_manager"]["workspaces"][0]["layout"];
    assert!(pane.to_string().contains("surface-terminal"));
    assert!(pane.to_string().contains("surface-browser"));
    assert!(
        pane.get("surfaces").is_some()
            || pane
                .get("Pane")
                .and_then(|pane| pane.get("surfaces"))
                .is_some(),
        "authoritative kind/runtime/metadata must persist per surface"
    );
}

#[test]
fn v2_lifecycle_transition_events_are_emitted_once_from_committed_state() {
    let previous = event_summary(
        vec![event_workspace(
            "workspace-1",
            "Workspace",
            0,
            &["surface-a"],
            Some("surface-a"),
        )],
        0,
    );
    let current = event_summary(
        vec![event_workspace(
            "workspace-1",
            "Workspace",
            0,
            &["surface-a", "surface-b"],
            Some("surface-b"),
        )],
        0,
    );
    let events = derived_session_event_specs(Some(&previous), &current);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.created")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.name == "surface.focused")
            .count(),
        1
    );
}

#[test]
fn regression_pane_focus_remains_scoped_to_resolved_workspace() {
    let snapshot = surface_move_snapshot();
    let target = json!({"workspace_id": "workspace-2", "pane_id": "pane-2"});
    assert_eq!(
        resolve_pane_focus_target(&snapshot, target.as_object().unwrap(), 0),
        Ok((1, 0, "pane-2".to_string()))
    );
    let wrong = json!({"workspace_id": "workspace-1", "pane_id": "pane-2"});
    assert_eq!(
        resolve_pane_focus_target(&snapshot, wrong.as_object().unwrap(), 0),
        Err(PaneFocusResolveError::PaneNotFound)
    );
}

#[test]
fn regression_surface_split_keeps_alias_kind_and_focus_decoding() {
    for alias in ["left", "l", "right", "r", "up", "u", "down", "d"] {
        let params = json!({"direction": alias});
        assert!(split_orientation_from_params(params.as_object().unwrap()).is_some());
    }
    let params = json!({"type": "Browser", "focus": "true"});
    assert_eq!(
        surface_kind_from_params(params.as_object().unwrap()),
        Some("browser".to_string())
    );
    assert_eq!(
        bool_param(params.as_object().unwrap(), &["focus"]),
        Some(true)
    );
}
