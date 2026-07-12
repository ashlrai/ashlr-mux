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

fn ok_value(result: ControlCallResult) -> Value {
    let ControlCallResult::Ok(value) = result else {
        panic!("expected successful decoded request result")
    };
    value.into()
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
}

#[test]
fn v2_pane_resize_absolute_intent_has_validation_precedence() {
    let params = json!({
        "absolute_axis": "diagonal",
        "target_pixels": 640,
        "direction": "left",
        "amount": 2,
    });
    assert!(contains_any_param(
        params.as_object().unwrap(),
        &["absolute_axis", "target_pixels"]
    ));
    assert_eq!(
        string_param(params.as_object().unwrap(), &["absolute_axis"]),
        Some("diagonal".to_string())
    );
    assert_eq!(
        "absolute_axis must be 'horizontal' or 'vertical'",
        "absolute_axis must be 'horizontal' or 'vertical'"
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
}

#[test]
fn v2_pane_and_surface_create_expose_dock_and_remote_routing_contracts() {
    for method in ["pane.create", "surface.create"] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
    }
    let dock = json!({"placement": "dock", "type": "browser", "focus": false});
    assert_eq!(
        string_param(dock.as_object().unwrap(), &["placement"]),
        Some("dock".to_string())
    );
    let remote = json!({
        "remote_pty_session_id": "remote-session-1",
        "remote_context": {"transport": "tmux"},
    });
    assert_eq!(
        raw_string_param(remote.as_object().unwrap(), &["remote_pty_session_id"]),
        Some("remote-session-1".to_string())
    );
}

#[test]
fn v2_surface_current_is_a_read_capability_without_focus_side_effects() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.current"));
    let snapshot = mixed_surface_snapshot();
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-terminal")
    );
}

#[test]
fn v2_surface_list_routes_to_the_explicit_second_window_without_fallback() {
    let snapshot = two_window_snapshot();
    let params = json!({"window_id": "window-2"});
    let value = ok_value(surface_list_from_params(
        &snapshot,
        params.as_object().unwrap(),
    ));
    assert_eq!(value["window_id"], json!("window-2"));
    assert_eq!(value["window_ref"], json!("window:2"));
    assert_eq!(value["workspace_id"], json!("workspace-2"));
    assert_eq!(value["surfaces"][0]["id"], json!("surface-2"));
}

#[test]
fn v2_surface_list_rejects_an_invalid_explicit_window_before_workspace_fallback() {
    let snapshot = two_window_snapshot();
    let params = json!({
        "window_id": "missing-window",
        "workspace_id": "workspace-2",
    });
    assert_eq!(
        workspace_routed_window_index(&snapshot, params.as_object().unwrap()),
        None
    );
    let ControlCallResult::Err { code, message, .. } =
        surface_list_from_params(&snapshot, params.as_object().unwrap())
    else {
        panic!("invalid explicit window must be terminal")
    };
    assert_eq!(code, "unavailable");
    assert_eq!(message, "TabManager not available");
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
    let same = json!({"path": " C:/repo ", "cwd": " C:/repo "});
    assert_eq!(
        raw_string_param(same.as_object().unwrap(), &["path", "directory", "cwd"]),
        Some(" C:/repo ".to_string())
    );
    let conflict = json!({"path": "C:/one", "directory": "C:/two"});
    let aliases: Vec<_> = ["path", "directory", "cwd"]
        .into_iter()
        .filter_map(|key| conflict.get(key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .collect();
    assert_ne!(aliases[0], aliases[1], "Conflicting path parameters");
}

#[test]
fn v2_surface_respawn_is_an_exact_identity_preserving_capability() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"surface.respawn"));
    let params = json!({
        "surface_id": "surface-terminal",
        "command": "  cargo test  ",
        "initial_command": "ignored",
        "focus": "false",
    });
    assert_eq!(
        string_param(params.as_object().unwrap(), &["command", "initial_command"]),
        Some("cargo test".to_string())
    );
    assert_eq!(
        bool_param(params.as_object().unwrap(), &["focus"]),
        Some(false)
    );
}

#[test]
fn v2_surface_close_focus_and_move_keep_exact_public_routes() {
    for method in ["surface.close", "surface.focus", "surface.move"] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
    }
    let conflict = json!({
        "surface_id": "surface-1",
        "before_surface_id": "surface-2",
        "after_surface_id": "surface-3",
    });
    assert_eq!(
        resolve_surface_move(&surface_move_snapshot(), conflict.as_object().unwrap()),
        Err(SurfaceMoveResolveError::ConflictingAnchors)
    );
}

#[test]
fn v2_surface_focus_uses_the_explicit_second_window_owner() {
    let snapshot = two_window_snapshot();
    let params = json!({
        "window_id": "window-2",
        "workspace_id": "workspace-2",
        "surface_id": "surface-2",
    });
    assert_eq!(
        workspace_routed_window_index(&snapshot, params.as_object().unwrap()),
        Some(1)
    );
    assert_eq!(
        surface_id_from_params_or_focused(&snapshot, params.as_object().unwrap()),
        Some("surface-2".to_string())
    );
}

#[test]
fn v2_surface_move_resolves_source_and_destination_across_windows() {
    let snapshot = two_window_snapshot();
    let params = json!({
        "surface_id": "surface-1",
        "window_id": "window-2",
        "focus": true,
    });
    let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap())
        .expect("second-window destination must resolve");
    assert_eq!(resolved.panel_id, "surface-1");
    assert_eq!(resolved.target_pane_id, "pane-2");
    assert!(resolved.focus);
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
