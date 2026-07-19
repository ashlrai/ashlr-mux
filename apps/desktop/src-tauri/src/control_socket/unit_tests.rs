use super::*;
use std::collections::BTreeSet;
static ENV_LOCK: Mutex<()> = Mutex::new(());
use cmux_core::session::SessionPanelShellActivitySnapshot;
use cmux_core::session::{
    AgentLaunchCommandSnapshot, AppSessionSnapshot, SessionGitBranchSnapshot,
    SessionPaneLayoutSnapshot, SessionPanelGitBranchSnapshot, SessionPanelListeningPortsSnapshot,
    SessionPanelPinSnapshot, SessionPanelPullRequestSnapshot, SessionPanelRestorableAgentSnapshot,
    SessionPanelTerminalStartupSnapshot, SessionPanelTitleSnapshot, SessionPanelTtySnapshot,
    SessionPanelUnreadSnapshot, SessionPullRequestStatusSnapshot, SessionRestorableAgentSnapshot,
    SessionTabManagerSnapshot, SessionWindowSnapshot, SessionWorkspaceAgentPidSnapshot,
    SessionWorkspaceGroupSnapshot, SessionWorkspaceRemoteDaemonSnapshot,
    SessionWorkspaceRemoteProxySnapshot, SessionWorkspaceRemoteSnapshot,
    SessionWorkspaceSidebarLogEntrySnapshot, SessionWorkspaceSidebarMetadataBlockSnapshot,
    SessionWorkspaceSidebarMetadataSnapshot, SessionWorkspaceSidebarProgressSnapshot,
    SessionWorkspaceSidebarStatusSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
};

fn test_snapshot() -> AppSessionSnapshot {
    AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 0,
        windows: vec![SessionWindowSnapshot {
            window_id: Some("window-1".to_string()),
            selected_workspace_id: None,
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![SessionWorkspaceSnapshot {
                    workspace_id: Some("workspace-1".to_string()),
                    process_title: "shell".to_string(),
                    custom_title: Some("Phoenix".to_string()),
                    current_directory: Some("C:/repo".to_string()),
                    focused_panel_id: Some("surface-1".to_string()),
                    layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
                        SessionPaneLayoutSnapshot {
                            pane_id: Some("pane-1".to_string()),
                            panel_ids: vec!["surface-1".to_string()],
                            selected_panel_id: Some("surface-1".to_string()),
                            surface_kind: None,
                            markdown_file_path: None,
                            file_path: None,
                            diff_viewer_token: None,
                            diff_viewer_request_path: None,
                            browser_url: None,
                            browser_proxy_url: None,
                            browser_back_history: None,
                            browser_forward_history: None,
                            browser_omnibar_visible: None,
                            browser_focus_mode_active: None,
                            browser_developer_tools_visible: None,
                            browser_developer_tools_panel: None,
                            browser_page_zoom: None,
                        },
                    )),
                    ..Default::default()
                }],
                workspace_groups: None,
            },
        }],
    }
}

#[test]
fn resolved_same_title_rename_still_produces_one_rename_event_spec() {
    let snapshot = test_snapshot();

    let event = resolved_workspace_rename_event_spec(&snapshot, 0, 0).unwrap();

    assert_eq!(event.name, "workspace.renamed");
    assert_eq!(event.workspace_id.as_deref(), Some("workspace-1"));
    assert_eq!(event.payload["title"], json!("Phoenix"));
    assert_eq!(event.payload["previous_title"], json!("Phoenix"));
}

fn surface_move_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let source = &mut snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(source_pane) =
        source.layout.as_mut().expect("source layout")
    else {
        unreachable!();
    };
    source_pane.panel_ids.push("surface-2".to_string());

    let mut destination = source.clone();
    destination.workspace_id = Some("workspace-2".to_string());
    let SessionWorkspaceLayoutSnapshot::Pane(destination_pane) =
        destination.layout.as_mut().expect("destination layout")
    else {
        unreachable!();
    };
    destination_pane.pane_id = Some("pane-2".to_string());
    destination_pane.panel_ids = vec!["surface-3".to_string(), "surface-4".to_string()];
    destination_pane.selected_panel_id = Some("surface-3".to_string());
    snapshot.windows[0].tab_manager.workspaces.push(destination);
    snapshot
}

#[test]
fn surface_move_resolver_matches_canonical_destination_precedence() {
    let snapshot = surface_move_snapshot();
    let params = serde_json::json!({
        "surface_id": "surface-1",
        "before_surface_id": "surface-4",
        "pane_id": "pane-1",
        "workspace_id": "workspace-1",
        "index": 99,
        "focus": true,
    });
    let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
    assert_eq!(
        resolved,
        SurfaceMoveResolution {
            source_workspace_index: 0,
            panel_id: "surface-1".to_string(),
            target_workspace_index: 1,
            target_pane_id: "pane-2".to_string(),
            destination_index: Some(1),
            focus: true,
        }
    );

    let params = serde_json::json!({
        "surface_id": "surface-1",
        "pane_id": "pane-2",
        "workspace_id": "workspace-1",
        "index": 2,
    });
    let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
    assert_eq!(resolved.target_workspace_index, 1);
    assert_eq!(resolved.target_pane_id, "pane-2");
    assert_eq!(resolved.destination_index, Some(2));

    let params = serde_json::json!({
        "surface_id": "surface-1",
        "workspace_id": "workspace-2",
    });
    let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
    assert_eq!(resolved.target_workspace_index, 1);
    assert_eq!(resolved.target_pane_id, "pane-2");

    let conflict = serde_json::json!({
        "surface_id": "surface-1",
        "before_surface_id": "surface-3",
        "after_surface_id": "surface-4",
    });
    assert_eq!(
        resolve_surface_move(&snapshot, conflict.as_object().unwrap()),
        Err(SurfaceMoveResolveError::ConflictingAnchors)
    );
}

#[test]
fn pane_join_source_resolves_explicit_surface_or_selected_pane_surface() {
    let snapshot = surface_move_snapshot();
    let direct = serde_json::json!({"surface_id": "surface-2"});
    assert_eq!(
        resolve_pane_join_source(&snapshot, direct.as_object().unwrap()),
        Ok("surface-2".to_string())
    );
    let by_pane = serde_json::json!({"pane_id": "pane-2"});
    assert_eq!(
        resolve_pane_join_source(&snapshot, by_pane.as_object().unwrap()),
        Ok("surface-3".to_string())
    );
    let by_ref = serde_json::json!({
        "workspace_ref": "workspace:2",
        "pane_ref": "pane:1",
    });
    assert_eq!(
        resolve_pane_join_source(&snapshot, by_ref.as_object().unwrap()),
        Ok("surface-3".to_string())
    );
    assert_eq!(
        resolve_pane_join_source(&snapshot, &serde_json::Map::new()),
        Err(PaneJoinSourceError::Missing)
    );
    let missing = serde_json::json!({"pane_id": "missing"});
    assert_eq!(
        resolve_pane_join_source(&snapshot, missing.as_object().unwrap()),
        Err(PaneJoinSourceError::SourcePaneUnresolved(
            "missing".to_string()
        ))
    );
}

#[test]
fn resize_pane_resolves_id_ref_or_persisted_focus() {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-1".to_string());

    let by_id = json!({"pane_id": "pane-1"});
    assert_eq!(
        resolve_resize_pane(workspace, by_id.as_object().unwrap()),
        Some((0, "pane-1".to_string()))
    );
    let by_ref = json!({"pane_ref": "pane:1"});
    assert_eq!(
        resolve_resize_pane(workspace, by_ref.as_object().unwrap()),
        Some((0, "pane-1".to_string()))
    );
    assert_eq!(
        resolve_resize_pane(workspace, &serde_json::Map::new()),
        Some((0, "pane-1".to_string()))
    );

    workspace.focused_panel_id = None;
    assert_eq!(
        resolve_resize_pane(workspace, &serde_json::Map::new()),
        None
    );
}

#[test]
fn pane_list_geometry_walks_nested_splits_in_leaf_order() {
    let base = test_snapshot();
    let SessionWorkspaceLayoutSnapshot::Pane(base_pane) = base.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    else {
        unreachable!()
    };
    let pane = |id: &str, surface: &str| {
        let mut pane = base_pane.clone();
        pane.pane_id = Some(id.to_string());
        pane.panel_ids = vec![surface.to_string()];
        pane.selected_panel_id = Some(surface.to_string());
        SessionWorkspaceLayoutSnapshot::Pane(pane)
    };
    let layout =
        SessionWorkspaceLayoutSnapshot::Split(cmux_core::session::SessionSplitLayoutSnapshot {
            split_id: Some("root".to_string()),
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.6,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Split(
                cmux_core::session::SessionSplitLayoutSnapshot {
                    split_id: Some("inner".to_string()),
                    orientation: SessionSplitOrientation::Vertical,
                    divider_position: 0.25,
                    first: Box::new(pane("pane-a", "surface-a")),
                    second: Box::new(pane("pane-b", "surface-b")),
                },
            )),
            second: Box::new(pane("pane-c", "surface-c")),
        });
    let mut rows = Vec::new();
    pane_frames(
        &layout,
        PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        },
        &mut rows,
    );
    assert_eq!(
        rows.iter()
            .map(|(pane, _)| pane.pane_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["pane-a", "pane-b", "pane-c"]
    );
    assert_eq!((rows[0].1.width, rows[0].1.height), (600.0, 200.0));
    assert_eq!((rows[1].1.x, rows[1].1.y), (0.0, 200.0));
    assert_eq!((rows[1].1.width, rows[1].1.height), (600.0, 600.0));
    assert_eq!((rows[2].1.x, rows[2].1.width), (600.0, 400.0));
}

#[test]
fn pane_surfaces_target_resolves_global_id_scoped_ref_or_focus() {
    let mut snapshot = surface_move_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("surface-1".to_string());
    let by_id = json!({"pane_id":"pane-2"});
    assert_eq!(
        resolve_pane_surfaces_target(&snapshot, by_id.as_object().unwrap(), 0),
        Some((1, 0, "pane-2".to_string()))
    );
    let by_ref = json!({"workspace_ref":"workspace:2", "pane_ref":"pane:1"});
    assert_eq!(
        resolve_pane_surfaces_target(&snapshot, by_ref.as_object().unwrap(), 0),
        Some((1, 0, "pane-2".to_string()))
    );
    assert_eq!(
        resolve_pane_surfaces_target(&snapshot, &serde_json::Map::new(), 0),
        Some((0, 0, "pane-1".to_string()))
    );
}

#[test]
fn pane_focus_target_is_scoped_to_the_resolved_workspace() {
    let mut snapshot = surface_move_snapshot();
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(1);
    let selected = json!({"pane_id":"pane-2"});
    assert_eq!(
        resolve_pane_focus_target(&snapshot, selected.as_object().unwrap(), 0),
        Ok((1, 0, "pane-2".to_string()))
    );

    let scoped = json!({"workspace_ref":"workspace:1", "pane_ref":"pane:1"});
    assert_eq!(
        resolve_pane_focus_target(&snapshot, scoped.as_object().unwrap(), 0),
        Ok((0, 0, "pane-1".to_string()))
    );

    let wrong_workspace = json!({"workspace_ref":"workspace:1", "pane_id":"pane-2"});
    assert_eq!(
        resolve_pane_focus_target(&snapshot, wrong_workspace.as_object().unwrap(), 0),
        Err(PaneFocusResolveError::PaneNotFound)
    );
}

#[test]
fn workspace_window_move_resolves_refs_locally_and_ids_globally() {
    let mut snapshot = surface_move_snapshot();
    let destination = snapshot.windows[0]
        .tab_manager
        .workspaces
        .pop()
        .expect("destination workspace");
    snapshot.windows.push(SessionWindowSnapshot {
        window_id: Some("window-2".to_string()),
        selected_workspace_id: None,
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![destination],
            workspace_groups: None,
        },
    });

    let by_ref = serde_json::json!({"workspace_ref": "workspace:1"});
    assert_eq!(
        workspace_id_for_window_move(&snapshot, by_ref.as_object().unwrap()).as_deref(),
        Some("workspace-1")
    );
    let by_id = serde_json::json!({"workspace_id": "workspace-2"});
    assert_eq!(
        workspace_id_for_window_move(&snapshot, by_id.as_object().unwrap()).as_deref(),
        Some("workspace-2")
    );
    assert_eq!(
        global_surface_location(&snapshot, "surface-3"),
        Some((1, 0))
    );
    assert_eq!(global_pane_location(&snapshot, "pane-2"), Some((1, 0, 0)));
    assert_eq!(
        split_off_workspace_index(&snapshot, by_ref.as_object().unwrap(), 0),
        Some(Some(0))
    );
}

#[test]
fn custom_sidebar_action_reply_uses_native_bridge_envelope() {
    let ok_reply = custom_sidebar_action_reply(ControlCallResult::Ok(
        JsonValue::try_from(json!({ "accepted": true })).expect("json value"),
    ));
    assert_eq!(ok_reply["ok"], json!(true));
    assert_eq!(ok_reply["value"]["accepted"], json!(true));

    let err_reply = custom_sidebar_action_reply(ControlCallResult::Err {
        code: "invalid_params".to_string(),
        message: "bad action".to_string(),
        data: Some(JsonValue::try_from(json!({ "field": "method" })).expect("json value")),
    });
    assert_eq!(err_reply["ok"], json!(false));
    assert_eq!(err_reply["error"]["code"], json!("invalid_params"));
    assert_eq!(err_reply["error"]["userMessage"], json!("bad action"));
    assert_eq!(err_reply["error"]["data"]["field"], json!("method"));
}

#[test]
fn custom_sidebar_action_policy_allows_safe_sidebar_methods() {
    for method in [
        "sidebar.list",
        "sidebar.select",
        "workspace.select",
        "workspace.set_status",
        "workspace.report_meta",
        "surface.focus",
        "extension.sidebar.snapshot",
    ] {
        assert!(
            custom_sidebar_action_policy_allows(method),
            "expected custom sidebar policy to allow {method}"
        );
    }
}

#[test]
fn custom_sidebar_action_policy_denies_dangerous_methods_with_data() {
    for method in [
        "workspace.close",
        "surface.close",
        "browser.eval",
        "browser.addscript",
        "debug.terminals",
        "workspace.remote.configure",
    ] {
        assert!(
            !custom_sidebar_action_policy_allows(method),
            "expected custom sidebar policy to deny {method}"
        );
        let reply = custom_sidebar_action_reply(custom_sidebar_action_denied(method, None));
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(
            reply["error"]["code"],
            json!("custom_sidebar_capability_denied")
        );
        assert_eq!(reply["error"]["data"]["method"], json!(method));
        assert_eq!(
            reply["error"]["data"]["policy"],
            json!(CUSTOM_SIDEBAR_ACTION_POLICY)
        );
        assert!(reply["error"]["data"]["allowed_methods"]
            .as_array()
            .is_some_and(|methods| methods.contains(&json!("workspace.select"))));
    }
}

#[test]
fn custom_sidebar_action_schema_validates_required_method_params() {
    let empty = serde_json::Map::new();
    let reply = custom_sidebar_action_reply(
        validate_custom_sidebar_action_schema("workspace.select", &empty)
            .expect_err("workspace.select should require a selector"),
    );
    assert_eq!(reply["ok"], json!(false));
    assert_eq!(
        reply["error"]["code"],
        json!("custom_sidebar_action_schema_invalid")
    );
    assert_eq!(reply["error"]["data"]["field"], json!("workspace"));
    assert_eq!(
        reply["error"]["data"]["accepted_keys"],
        json!(["workspace_id", "id", "workspace_ref", "ref"])
    );

    let params = json!({ "workspace_id": "workspace-1" })
        .as_object()
        .expect("object")
        .clone();
    assert!(validate_custom_sidebar_action_schema("workspace.select", &params).is_ok());

    let params = json!({ "key": "deploy", "value": "running", "priority": "10" })
        .as_object()
        .expect("object")
        .clone();
    assert!(validate_custom_sidebar_action_schema("workspace.set_status", &params).is_ok());

    let params = json!({ "key": "deploy", "value": "running", "priority": "high" })
        .as_object()
        .expect("object")
        .clone();
    let reply = custom_sidebar_action_reply(
        validate_custom_sidebar_action_schema("workspace.set_status", &params)
            .expect_err("priority should be an integer"),
    );
    assert_eq!(reply["error"]["data"]["field"], json!("priority"));
    assert_eq!(
        reply["error"]["data"]["expected"],
        json!("integer or integer string")
    );
}

#[test]
fn custom_sidebar_action_schema_advertises_authoring_contract() {
    let catalog = custom_sidebar_action_schema_catalog();
    assert_eq!(
        catalog["version"],
        json!(CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION)
    );
    assert!(catalog["methods"].as_array().is_some_and(|methods| methods
        .iter()
        .any(|method| method["method"] == json!("workspace.set_status"))));
    assert_eq!(
        catalog["selector_keys"]["surface"],
        json!(["surface_id", "panel_id", "id", "surface_ref", "ref"])
    );
}

#[test]
fn workspace_list_payload_matches_control_shape() {
    let payload = workspace_list_payload(&test_snapshot());
    assert_eq!(payload["window_id"], json!("window-1"));
    assert_eq!(payload["workspaces"][0]["id"], json!("workspace-1"));
    assert_eq!(payload["workspaces"][0]["ref"], json!("workspace:1"));
    assert_eq!(payload["workspaces"][0]["title"], json!("Phoenix"));
    assert_eq!(payload["workspaces"][0]["selected"], json!(true));
    assert_eq!(
        payload["workspaces"][0]["current_directory"],
        json!("C:/repo")
    );
    assert_eq!(
        payload["workspaces"][0]["initial_terminal_command"],
        Value::Null
    );
    assert_eq!(
        payload["workspaces"][0]["initial_terminal_input"],
        Value::Null
    );
    assert_eq!(
        payload["workspaces"][0]["initial_terminal_environment"],
        Value::Null
    );
    assert_eq!(payload["workspaces"][0]["zoomed_panel_id"], Value::Null);
    assert_eq!(
        payload["workspaces"][0]["restorable_agent_panels"],
        json!([])
    );
    assert_eq!(payload["workspaces"][0]["git_branch"], Value::Null);
    assert_eq!(payload["workspaces"][0]["panel_git_branches"], Value::Null);
    assert_eq!(payload["workspaces"][0]["panel_pull_requests"], Value::Null);
    assert_eq!(payload["workspaces"][0]["sidebar_progress"], Value::Null);
    assert_eq!(
        payload["workspaces"][0]["sidebar_status_entries"],
        Value::Null
    );
    assert_eq!(
        payload["workspaces"][0]["sidebar_metadata_entries"],
        Value::Null
    );
    assert_eq!(
        payload["workspaces"][0]["sidebar_metadata_blocks"],
        Value::Null
    );
    assert_eq!(payload["workspaces"][0]["sidebar_log_entries"], Value::Null);
    assert_eq!(payload["workspace_groups"], json!([]));
}

#[test]
fn extension_sidebar_snapshot_projects_documented_authoring_data() {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.custom_description = Some("Ship parity".to_string());
    workspace.listening_ports = Some(vec![3000]);
    workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "surface-1".to_string(),
        ports: vec![5173],
    }]);
    workspace.git_branch = Some(SessionGitBranchSnapshot {
        branch: "main".to_string(),
        is_dirty: false,
    });
    workspace.panel_git_branches = Some(vec![SessionPanelGitBranchSnapshot {
        panel_id: "surface-1".to_string(),
        branch: "feature/sidebar".to_string(),
        is_dirty: true,
    }]);
    workspace.panel_unreads = Some(vec![SessionPanelUnreadSnapshot {
        panel_id: "surface-1".to_string(),
        is_unread: true,
        unread_at: Some(10),
    }]);
    workspace.panel_pull_requests = Some(vec![SessionPanelPullRequestSnapshot {
        panel_id: "surface-1".to_string(),
        number: 42,
        label: "Review".to_string(),
        url: "https://github.com/example/repo/pull/42".to_string(),
        status: SessionPullRequestStatusSnapshot::Open,
        branch: Some("feature/sidebar".to_string()),
        is_stale: false,
    }]);
    workspace.sidebar_progress = Some(SessionWorkspaceSidebarProgressSnapshot {
        value: 0.5,
        label: Some("halfway".to_string()),
    });

    let payload = extension_sidebar_snapshot_payload(&snapshot);
    assert_eq!(
        payload["protocol"],
        json!("cmux-extension-sidebar-snapshot")
    );
    assert_eq!(payload["selected_workspace_id"], json!("workspace-1"));
    assert_eq!(payload["selectedId"], json!("workspace-1"));
    assert_eq!(payload["selectedTitle"], json!("Phoenix"));
    assert_eq!(payload["workspaceCount"], json!(1));
    assert_eq!(payload["unreadTotal"], json!(1));

    let workspace = &payload["workspaces"][0];
    assert_eq!(workspace["id"], json!("workspace-1"));
    assert_eq!(workspace["directory"], json!("C:/repo"));
    assert_eq!(workspace["root_path"], json!("C:/repo"));
    assert_eq!(workspace["ports"], json!([3000, 5173]));
    assert_eq!(workspace["portCount"], json!(2));
    assert_eq!(workspace["tabCount"], json!(1));
    assert_eq!(workspace["unread"], json!(1));
    assert_eq!(workspace["branch"], json!("feature/sidebar"));
    assert_eq!(workspace["dirty"], json!(true));
    assert_eq!(workspace["branch_summary"], json!("feature/sidebar*"));
    assert_eq!(workspace["pr"]["number"], json!(42));
    assert_eq!(workspace["pr"]["status"], json!("open"));
    assert_eq!(
        workspace["pull_request_urls"],
        json!(["https://github.com/example/repo/pull/42"])
    );
    assert_eq!(workspace["progress"]["label"], json!("halfway"));
    assert_eq!(workspace["tabs"][0]["id"], json!("surface-1"));
    assert_eq!(workspace["tabs"][0]["directory"], json!("C:/repo"));
    assert_eq!(workspace["tabs"][0]["ports"], json!([5173]));
    assert_eq!(workspace["tabs"][0]["branch"], json!("feature/sidebar"));
    assert_eq!(workspace["tabs"][0]["dirty"], json!(true));
    assert_eq!(
        workspace["panel_directories"]["surface-1"],
        json!("C:/repo")
    );
    let data = &payload["data"];
    assert_eq!(data["workspaceCount"], json!(1));
    assert_eq!(data["selectedId"], json!("workspace-1"));
    assert_eq!(data["selectedTitle"], json!("Phoenix"));
    assert_eq!(data["unreadTotal"], json!(1));
    assert_eq!(data["events"]["latest"], Value::Null);
    assert!(data["clock"]["time"].as_str().is_some());
    assert!(data["clock"]["epoch"].as_i64().is_some());
    let data_workspace = &data["workspaces"][0];
    assert_eq!(data_workspace["id"], json!("workspace-1"));
    assert_eq!(data_workspace["title"], json!("Phoenix"));
    assert_eq!(data_workspace["selected"], json!(true));
    assert_eq!(data_workspace["pinned"], json!(false));
    assert_eq!(data_workspace["index"], json!(0));
    assert_eq!(data_workspace["directory"], json!("C:/repo"));
    assert_eq!(data_workspace["ports"], json!([3000, 5173]));
    assert_eq!(data_workspace["portCount"], json!(2));
    assert_eq!(data_workspace["unread"], json!(1));
    assert_eq!(data_workspace["tabCount"], json!(1));
    assert_eq!(data_workspace["description"], json!("Ship parity"));
    assert_eq!(data_workspace["branch"], json!("feature/sidebar"));
    assert_eq!(data_workspace["dirty"], json!(true));
    assert_eq!(data_workspace["pr"]["number"], json!(42));
    assert_eq!(data_workspace["progress"]["label"], json!("halfway"));
    assert_eq!(data_workspace["latestMessage"], Value::Null);
    assert_eq!(data_workspace["latestPrompt"], Value::Null);
    assert_eq!(data_workspace["latestAt"], Value::Null);
    assert_eq!(data_workspace["tabs"][0]["id"], json!("surface-1"));
    assert_eq!(data_workspace["tabs"][0]["title"], json!("terminal"));
    assert_eq!(data_workspace["tabs"][0]["focused"], json!(true));
    assert_eq!(data_workspace["tabs"][0]["ports"], json!([5173]));
    assert_eq!(
        data_workspace["tabs"][0]["branch"],
        json!("feature/sidebar")
    );
    assert_eq!(data_workspace["tabs"][0]["dirty"], json!(true));
    assert_eq!(payload["events"]["latest"], Value::Null);
    assert_eq!(payload["events"]["recent"], json!([]));
}

#[test]
fn extension_sidebar_snapshot_includes_event_context_for_eventbridge_bootstrap() {
    let events = extension_sidebar_events_context_from_retained(
        "boot-1".to_string(),
        4,
        vec![
            json!({
                "type": "event",
                "seq": 1,
                "name": "session.changed",
                "category": "session",
            }),
            json!({
                "type": "event",
                "seq": 2,
                "name": "workspace.selected",
                "category": "workspace",
            }),
            json!({
                "type": "event",
                "seq": 3,
                "name": "surface.selected",
                "category": "surface",
            }),
        ],
    );

    let payload =
        extension_sidebar_snapshot_payload_with_events(&test_snapshot(), events, json!({}));

    assert_eq!(payload["seq"], json!(3));
    assert_eq!(payload["latest_seq"], json!(3));
    assert_eq!(payload["events"]["protocol"], json!("cmux-events"));
    assert_eq!(payload["events"]["boot_id"], json!("boot-1"));
    assert_eq!(payload["events"]["oldest_seq"], json!(1));
    assert_eq!(payload["events"]["next_seq"], json!(4));
    assert_eq!(payload["events"]["retained_count"], json!(3));
    assert_eq!(
        payload["events"]["latest"]["name"],
        json!("surface.selected")
    );
    assert_eq!(
        payload["events"]["category_counts"],
        json!({
            "session": 1,
            "surface": 1,
            "workspace": 1,
        })
    );
    assert_eq!(
        payload["events"]["name_counts"]["workspace.selected"],
        json!(1)
    );
    assert_eq!(payload["events"]["recent"].as_array().unwrap().len(), 3);
}

#[test]
fn custom_sidebar_validation_prefers_swift_and_reports_invalid_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("status-board.json"), "{}").expect("write json");
    fs::write(dir.path().join("status-board.swift"), "Text(\"Status\")").expect("write swift");
    fs::write(dir.path().join("broken.json"), "{").expect("write broken");
    fs::write(
        dir.path().join("status-board.manifest.json"),
        r#"{"trusted":true,"capabilities":["workspace.select","browser.eval"]}"#,
    )
    .expect("write manifest");

    let payload = match validate_custom_sidebars_in_dir(dir.path(), None) {
        ControlCallResult::Ok(value) => Value::from(value),
        other => panic!("expected validation payload, got {other:?}"),
    };

    assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-validation"));
    assert_eq!(payload["valid_count"], json!(1));
    assert_eq!(payload["invalid_count"], json!(1));
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["sidebars"][0]["name"], json!("broken"));
    assert_eq!(payload["sidebars"][0]["kind"], json!("json"));
    assert_eq!(payload["sidebars"][0]["valid"], json!(false));
    assert!(payload["sidebars"][0]["errors"][0]
        .as_str()
        .is_some_and(|error| error.contains("invalid JSON")));
    assert_eq!(payload["sidebars"][1]["name"], json!("status-board"));
    assert_eq!(payload["sidebars"][1]["kind"], json!("swift"));
    assert_eq!(payload["sidebars"][1]["valid"], json!(true));
    assert_eq!(payload["sidebars"][1]["manifest"]["trusted"], json!(true));
    assert_eq!(
        payload["sidebars"][1]["manifest"]["requested_methods"],
        json!(["browser.eval", "workspace.select"])
    );
    assert_eq!(
        payload["sidebars"][1]["manifest"]["allowed_requested_methods"],
        json!(["workspace.select"])
    );
    assert_eq!(
        payload["sidebars"][1]["manifest"]["denied_requested_methods"],
        json!(["browser.eval"])
    );
    assert!(payload["sidebars"][1]["shadowed_json_path"]
        .as_str()
        .is_some_and(|path| path.ends_with("status-board.json")));

    let named = match validate_custom_sidebars_in_dir(dir.path(), Some("status-board")) {
        ControlCallResult::Ok(value) => Value::from(value),
        other => panic!("expected named validation payload, got {other:?}"),
    };
    assert_eq!(named["name"], json!("status-board"));
    assert_eq!(named["sidebars"].as_array().map(Vec::len), Some(1));
}

#[test]
fn custom_sidebar_assets_are_minted_and_resolved_from_adjacent_asset_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let previous_dir = std::env::var_os("CMUX_SIDEBARS_DIR");
    let dir = tempfile::tempdir().expect("tempdir");
    unsafe {
        std::env::set_var("CMUX_SIDEBARS_DIR", dir.path());
    }

    let source_path = dir.path().join("ops.swift");
    let asset_dir = dir.path().join("ops.assets");
    fs::create_dir_all(asset_dir.join("icons")).expect("asset dir");
    fs::write(&source_path, "Image(\"logo\")").expect("write swift");
    fs::write(asset_dir.join("logo.png"), [137, 80, 78, 71]).expect("write png");
    fs::write(asset_dir.join("icons").join("badge.svg"), "<svg />").expect("write svg");
    fs::write(asset_dir.join("secret.txt"), "nope").expect("write text");

    let params = serde_json::Map::from_iter([(
        "source_path".to_string(),
        json!(source_path.to_string_lossy()),
    )]);
    let assets = custom_sidebar_asset_map_from_params(&params);
    assert!(assets["logo"]
        .as_str()
        .is_some_and(|url| url.starts_with("cmux-sidebar-asset://ops/logo.png?source=")));
    assert!(assets["logo.png"].as_str().is_some());
    assert!(assets["icons/badge"].as_str().is_some());
    assert!(assets.get("secret").is_none());

    let logo_url = assets["logo"].as_str().expect("logo url");
    let (resolved_path, mime) =
        resolve_custom_sidebar_asset_request(logo_url).expect("resolve logo");
    assert_eq!(
        resolved_path,
        fs::canonicalize(asset_dir.join("logo.png")).unwrap()
    );
    assert_eq!(mime, "image/png");
    assert!(
        resolve_custom_sidebar_asset_request(&logo_url.replace("logo.png", "../ops.swift"))
            .is_none()
    );

    match previous_dir {
        Some(value) => unsafe {
            std::env::set_var("CMUX_SIDEBARS_DIR", value);
        },
        None => unsafe {
            std::env::remove_var("CMUX_SIDEBARS_DIR");
        },
    }
}

#[test]
fn custom_sidebar_action_denial_can_include_manifest_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("ops.swift");
    fs::write(&source_path, "Text(\"Ops\")").expect("write swift");
    fs::write(
        dir.path().join("ops.manifest.json"),
        r#"{"trusted":false,"allowed_methods":["workspace.select","browser.eval"]}"#,
    )
    .expect("write manifest");

    let manifest =
        custom_sidebar_manifest_for_source(source_path.to_str()).expect("manifest summary");
    let reply =
        custom_sidebar_action_reply(custom_sidebar_action_denied("browser.eval", Some(manifest)));

    assert_eq!(reply["ok"], json!(false));
    assert_eq!(
        reply["error"]["code"],
        json!("custom_sidebar_capability_denied")
    );
    assert_eq!(
        reply["error"]["data"]["manifest"]["requested_methods"],
        json!(["browser.eval", "workspace.select"])
    );
    assert_eq!(
        reply["error"]["data"]["manifest"]["denied_requested_methods"],
        json!(["browser.eval"])
    );
}

#[test]
fn custom_sidebar_reload_payload_targets_only_valid_sidebars() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("ops.json"), "{}").expect("write valid json");
    fs::write(dir.path().join("broken.json"), "{").expect("write broken json");

    let validation = match validate_custom_sidebars_in_dir(dir.path(), None) {
        ControlCallResult::Ok(value) => Value::from(value),
        other => panic!("expected validation payload, got {other:?}"),
    };
    let payload = custom_sidebar_reload_payload(None, &validation);

    assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-reload"));
    assert_eq!(payload["event"], json!(CUSTOM_SIDEBAR_RELOAD_EVENT));
    assert_eq!(payload["all"], json!(true));
    assert_eq!(payload["name"], Value::Null);
    assert_eq!(payload["sidebars"].as_array().map(Vec::len), Some(1));
    assert_eq!(payload["sidebars"][0]["name"], json!("ops"));
    assert!(payload["paths"][0]
        .as_str()
        .is_some_and(|path| path.ends_with("ops.json")));

    let named_payload = custom_sidebar_reload_payload(Some("ops"), &validation);
    assert_eq!(named_payload["all"], json!(false));
    assert_eq!(named_payload["name"], json!("ops"));
}

#[test]
fn custom_sidebar_select_payload_describes_selected_sidebar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("ops.json");
    fs::write(&path, "{}").expect("write valid json");
    let candidate = custom_sidebar_candidate_for_name(dir.path(), "ops")
        .expect("discover")
        .expect("candidate");
    let validation = validate_custom_sidebar_candidate(&candidate);

    let payload = custom_sidebar_select_payload(&candidate, &validation);

    assert_eq!(payload["accepted"], json!(true));
    assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-select"));
    assert_eq!(payload["event"], json!(CUSTOM_SIDEBAR_SELECT_EVENT));
    assert_eq!(payload["name"], json!("ops"));
    assert_eq!(payload["kind"], json!("json"));
    assert!(payload["path"]
        .as_str()
        .is_some_and(|path| path.ends_with("ops.json")));
    assert_eq!(payload["sidebar"]["valid"], json!(true));
}

#[test]
fn workspace_list_payload_includes_sidebar_progress() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].sidebar_progress =
        Some(SessionWorkspaceSidebarProgressSnapshot {
            value: 0.5,
            label: Some("Building".to_string()),
        });

    let payload = workspace_list_payload(&snapshot);

    assert_eq!(
        payload["workspaces"][0]["sidebar_progress"],
        json!({"value": 0.5, "label": "Building"})
    );
}

#[test]
fn workspace_list_payload_includes_sidebar_status_and_log() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].sidebar_status_entries =
        Some(vec![SessionWorkspaceSidebarStatusSnapshot {
            key: "build".to_string(),
            value: "green".to_string(),
            priority: Some(80),
            updated_at: 10,
        }]);
    snapshot.windows[0].tab_manager.workspaces[0].sidebar_log_entries =
        Some(vec![SessionWorkspaceSidebarLogEntrySnapshot {
            level: "info".to_string(),
            message: "ship it".to_string(),
            created_at: 11,
        }]);

    let payload = workspace_list_payload(&snapshot);

    assert_eq!(
        payload["workspaces"][0]["sidebar_status_entries"],
        json!([{"key": "build", "value": "green", "priority": 80, "updated_at": 10}])
    );
    assert_eq!(
        payload["workspaces"][0]["sidebar_log_entries"],
        json!([{"level": "info", "message": "ship it", "created_at": 11}])
    );
}

#[test]
fn workspace_list_payload_includes_sidebar_metadata_entries_and_blocks() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].sidebar_metadata_entries =
        Some(vec![SessionWorkspaceSidebarMetadataSnapshot {
            key: "task".to_string(),
            value: "review".to_string(),
            icon: Some("text:CTX".to_string()),
            color: Some("blue".to_string()),
            url: Some("https://example.test/pr".to_string()),
            priority: Some(50),
            format: Some("markdown".to_string()),
            updated_at: 12,
        }]);
    snapshot.windows[0].tab_manager.workspaces[0].sidebar_metadata_blocks =
        Some(vec![SessionWorkspaceSidebarMetadataBlockSnapshot {
            key: "notes".to_string(),
            markdown: "**Ready**".to_string(),
            priority: Some(10),
            updated_at: 13,
        }]);

    let payload = workspace_list_payload(&snapshot);

    assert_eq!(
        payload["workspaces"][0]["sidebar_metadata_entries"],
        json!([{
            "key": "task",
            "value": "review",
            "icon": "text:CTX",
            "color": "blue",
            "url": "https://example.test/pr",
            "priority": 50,
            "format": "markdown",
            "updated_at": 12
        }])
    );
    assert_eq!(
        payload["workspaces"][0]["sidebar_metadata_blocks"],
        json!([{
            "key": "notes",
            "markdown": "**Ready**",
            "priority": 10,
            "updated_at": 13
        }])
    );
}

#[test]
fn workspace_list_payload_includes_panel_pull_requests() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].panel_pull_requests =
        Some(vec![SessionPanelPullRequestSnapshot {
            panel_id: "surface-1".to_string(),
            number: 42,
            label: "MR".to_string(),
            url: "https://gitlab.example/project/-/merge_requests/42".to_string(),
            status: SessionPullRequestStatusSnapshot::Open,
            branch: Some("feature/api".to_string()),
            is_stale: false,
        }]);

    let payload = workspace_list_payload(&snapshot);
    assert_eq!(
        payload["workspaces"][0]["panel_pull_requests"],
        json!([{
            "panel_id": "surface-1",
            "number": 42,
            "label": "MR",
            "url": "https://gitlab.example/project/-/merge_requests/42",
            "status": "open",
            "branch": "feature/api",
            "is_stale": false,
        }])
    );
}

#[test]
fn not_supported_errors_use_socket_contract_shape() {
    let result = not_supported("browser viewport override is not supported by WKWebView");
    match result {
        ControlCallResult::Err {
            code,
            message,
            data,
        } => {
            assert_eq!(code, "not_supported");
            assert!(message.contains("WKWebView"));
            assert_eq!(data, None);
        }
        other => panic!("expected not_supported error, got {other:?}"),
    }
}

#[test]
fn event_filters_match_name_and_category() {
    let event = json!({
        "name": "session.changed",
        "category": "session",
    });
    assert!(event_matches_filters(&event, &[], &[]));
    assert!(event_matches_filters(
        &event,
        &["session.changed".to_string()],
        &["session".to_string()]
    ));
    assert!(!event_matches_filters(
        &event,
        &["workspace.selected".to_string()],
        &[]
    ));
    assert!(!event_matches_filters(
        &event,
        &[],
        &["notification".to_string()]
    ));
}

#[test]
fn live_event_subscribers_receive_matching_frames_and_prune_closed_receivers() {
    let (workspace_sender, mut workspace_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let (surface_sender, mut surface_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let (closed_sender, closed_receiver) = cmux_ipc::stream_mpsc::unbounded_channel::<String>();
    drop(closed_receiver);
    let mut subscribers = vec![
        EventSubscriber {
            sender: workspace_sender,
            names: Vec::new(),
            categories: vec!["workspace".to_string()],
        },
        EventSubscriber {
            sender: surface_sender,
            names: Vec::new(),
            categories: vec!["surface".to_string()],
        },
        EventSubscriber {
            sender: closed_sender,
            names: Vec::new(),
            categories: vec!["workspace".to_string()],
        },
    ];
    let event = json!({
        "name": "workspace.selected",
        "category": "workspace",
    });

    fan_out_event_to_subscribers(&mut subscribers, &event, r#"{"seq":1}"#);

    assert_eq!(workspace_receiver.try_recv().unwrap(), r#"{"seq":1}"#);
    assert!(surface_receiver.try_recv().is_err());
    assert_eq!(subscribers.len(), 2);
}

fn event_summary(
    workspaces: Vec<WorkspaceEventSummary>,
    selected_index: usize,
) -> SessionEventSummary {
    SessionEventSummary {
        window_id: Some("window-1".to_string()),
        selected_workspace_id: workspaces
            .get(selected_index)
            .and_then(|workspace| workspace.id.clone()),
        selected_workspace_index: Some(selected_index),
        workspaces,
    }
}

fn event_workspace(
    id: &str,
    title: &str,
    index: usize,
    surfaces: &[&str],
    selected_surface_id: Option<&str>,
) -> WorkspaceEventSummary {
    WorkspaceEventSummary {
        key: id.to_string(),
        id: Some(id.to_string()),
        title: title.to_string(),
        index,
        panes: vec![event_pane("pane-1", 0, surfaces, selected_surface_id)],
        surface_ids: surfaces.iter().map(|surface| surface.to_string()).collect(),
        selected_surface_id: selected_surface_id.map(str::to_string),
        sidebar: WorkspaceSidebarEventSummary::default(),
    }
}

fn event_pane(
    id: &str,
    index: usize,
    surfaces: &[&str],
    selected_surface_id: Option<&str>,
) -> PaneEventSummary {
    PaneEventSummary {
        key: id.to_string(),
        id: Some(id.to_string()),
        index,
        surface_ids: surfaces.iter().map(|surface| surface.to_string()).collect(),
        selected_surface_id: selected_surface_id.map(str::to_string),
    }
}

fn sidebar_map(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn event_names(events: &[DerivedEventSpec]) -> Vec<&'static str> {
    events.iter().map(|event| event.name).collect()
}

#[test]
fn derived_session_events_bootstrap_current_workspace_and_surface_state() {
    let current = event_summary(
        vec![event_workspace(
            "workspace-1",
            "Phoenix",
            0,
            &["surface-1"],
            Some("surface-1"),
        )],
        0,
    );

    let events = derived_session_event_specs(None, &current);

    assert_eq!(
        event_names(&events),
        vec![
            "session.changed",
            "workspace.created",
            "pane.created",
            "pane.focused",
            "surface.created",
            "workspace.selected",
            "surface.selected",
        ]
    );
    assert_eq!(events[1].payload["workspace_ref"], json!("workspace:1"));
    assert_eq!(events[2].payload["pane_ref"], json!("pane:1"));
    assert_eq!(events[4].payload["surface_ref"], json!("surface:1"));
}

#[test]
fn derived_session_events_capture_workspace_and_surface_diffs() {
    let previous = event_summary(
        vec![
            event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a")),
            event_workspace("workspace-b", "Beta", 1, &["surface-b"], Some("surface-b")),
        ],
        0,
    );
    let current = event_summary(
        vec![
            event_workspace(
                "workspace-b",
                "Beta Prime",
                0,
                &["surface-b", "surface-c"],
                Some("surface-c"),
            ),
            event_workspace("workspace-a", "Alpha", 1, &[], None),
        ],
        0,
    );

    let events = derived_session_event_specs(Some(&previous), &current);
    let names = event_names(&events);

    assert!(names.contains(&"session.changed"));
    assert!(names.contains(&"workspace.selected"));
    assert!(names.contains(&"workspace.renamed"));
    assert!(names.contains(&"workspace.reordered"));
    assert!(names.contains(&"surface.created"));
    assert!(names.contains(&"surface.closed"));
    assert!(names.contains(&"surface.selected"));
    let selected = events
        .iter()
        .find(|event| event.name == "workspace.selected")
        .expect("workspace selected event");
    assert_eq!(
        selected.payload["previous_workspace_id"],
        json!("workspace-a")
    );
    let renamed = events
        .iter()
        .find(|event| event.name == "workspace.renamed")
        .expect("workspace renamed event");
    assert_eq!(renamed.payload["previous_title"], json!("Beta"));
    let reordered = events
        .iter()
        .find(|event| event.name == "workspace.reordered")
        .expect("workspace reordered event");
    assert_eq!(
        reordered.payload["workspace_ids"],
        json!(["workspace-b", "workspace-a"])
    );
}

#[test]
fn derived_session_events_close_surfaces_when_workspace_closes() {
    let previous = event_summary(
        vec![event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a"],
            Some("surface-a"),
        )],
        0,
    );
    let current = event_summary(
        vec![event_workspace(
            "workspace-b",
            "Beta",
            0,
            &["surface-b"],
            Some("surface-b"),
        )],
        0,
    );

    let events = derived_session_event_specs(Some(&previous), &current);
    let names = event_names(&events);

    assert!(names.contains(&"workspace.created"));
    assert!(names.contains(&"workspace.closed"));
    assert!(names.contains(&"surface.created"));
    assert!(names.contains(&"surface.closed"));
    let closed_surface = events
        .iter()
        .find(|event| event.name == "surface.closed")
        .expect("surface closed event");
    assert_eq!(closed_surface.surface_id, Some("surface-a".to_string()));
}

#[test]
fn derived_session_events_emit_one_move_without_close_create_duplicates() {
    let previous = event_summary(
        vec![
            event_workspace(
                "workspace-a",
                "Alpha",
                0,
                &["surface-a", "surface-b"],
                Some("surface-a"),
            ),
            event_workspace("workspace-b", "Beta", 1, &["surface-c"], Some("surface-c")),
        ],
        0,
    );
    let current = event_summary(
        vec![
            event_workspace("workspace-a", "Alpha", 0, &["surface-b"], Some("surface-b")),
            event_workspace(
                "workspace-b",
                "Beta",
                1,
                &["surface-c", "surface-a"],
                Some("surface-a"),
            ),
        ],
        1,
    );
    let events = derived_session_event_specs(Some(&previous), &current);
    assert!(!events.iter().any(|event| {
        event.name == "surface.moved" && event.surface_id.as_deref() == Some("surface-a")
    }));
    assert!(!events.iter().any(|event| {
        matches!(event.name, "surface.created" | "surface.closed")
            && event.surface_id.as_deref() == Some("surface-a")
    }));
}

#[test]
fn derived_session_events_capture_pane_lifecycle_and_focus() {
    let mut previous_workspace = event_workspace(
        "workspace-a",
        "Alpha",
        0,
        &["surface-a", "surface-b"],
        Some("surface-a"),
    );
    previous_workspace.panes = vec![event_pane(
        "pane-a",
        0,
        &["surface-a", "surface-b"],
        Some("surface-a"),
    )];
    let previous = event_summary(vec![previous_workspace], 0);

    let mut current_workspace = event_workspace(
        "workspace-a",
        "Alpha",
        0,
        &["surface-a", "surface-b", "surface-c"],
        Some("surface-b"),
    );
    current_workspace.panes = vec![
        event_pane("pane-a", 0, &["surface-a", "surface-b"], Some("surface-b")),
        event_pane("pane-b", 1, &["surface-c"], Some("surface-c")),
    ];
    let current = event_summary(vec![current_workspace], 0);

    let events = derived_session_event_specs(Some(&previous), &current);
    let names = event_names(&events);

    assert!(names.contains(&"pane.focused"));
    assert!(names.contains(&"pane.created"));
    let focused = events
        .iter()
        .find(|event| event.name == "pane.focused" && event.payload["pane_id"] == json!("pane-a"))
        .expect("pane focused");
    assert_eq!(focused.category, "pane");
    assert_eq!(focused.payload["previous_surface_id"], json!("surface-a"));
    assert_eq!(focused.payload["selected_surface_id"], json!("surface-b"));
    let created = events
        .iter()
        .find(|event| event.name == "pane.created" && event.payload["pane_id"] == json!("pane-b"))
        .expect("pane created");
    assert_eq!(created.payload["pane_ref"], json!("pane:2"));
}

#[test]
fn derived_session_events_close_panes_when_removed() {
    let mut previous_workspace = event_workspace(
        "workspace-a",
        "Alpha",
        0,
        &["surface-a", "surface-b"],
        Some("surface-a"),
    );
    previous_workspace.panes = vec![
        event_pane("pane-a", 0, &["surface-a"], Some("surface-a")),
        event_pane("pane-b", 1, &["surface-b"], Some("surface-b")),
    ];
    let previous = event_summary(vec![previous_workspace], 0);

    let mut current_workspace =
        event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
    current_workspace.panes = vec![event_pane("pane-a", 0, &["surface-a"], Some("surface-a"))];
    let current = event_summary(vec![current_workspace], 0);

    let events = derived_session_event_specs(Some(&previous), &current);

    let closed = events
        .iter()
        .find(|event| event.name == "pane.closed")
        .expect("pane closed");
    assert_eq!(closed.payload["pane_id"], json!("pane-b"));
    assert_eq!(closed.payload["pane_ref"], json!("pane:2"));
}

#[test]
fn derived_session_events_capture_sidebar_metadata_updates() {
    let previous = event_summary(
        vec![event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a"],
            Some("surface-a"),
        )],
        0,
    );
    let mut workspace =
        event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
    workspace.sidebar.progress = Some(json!({"value": 0.5, "label": "Half"}));
    workspace.sidebar.status_entries =
        sidebar_map(&[("build", json!({"key": "build", "value": "green"}))]);
    workspace.sidebar.metadata_entries =
        sidebar_map(&[("task", json!({"key": "task", "value": "review"}))]);
    workspace.sidebar.metadata_blocks =
        sidebar_map(&[("notes", json!({"key": "notes", "markdown": "Ready"}))]);
    workspace.sidebar.log_entries = vec![json!({"level": "info", "message": "ship it"})];
    let current = event_summary(vec![workspace], 0);

    let events = derived_session_event_specs(Some(&previous), &current);
    let names = event_names(&events);

    assert!(names.contains(&"sidebar.progress.updated"));
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "sidebar.metadata.updated")
            .count(),
        3
    );
    assert!(names.contains(&"sidebar.log.appended"));
    let progress = events
        .iter()
        .find(|event| event.name == "sidebar.progress.updated")
        .expect("progress event");
    assert_eq!(progress.category, "sidebar");
    assert_eq!(progress.payload["kind"], json!("progress"));
    let status = events
        .iter()
        .find(|event| {
            event.name == "sidebar.metadata.updated" && event.payload["kind"] == json!("status")
        })
        .expect("status event");
    assert_eq!(status.payload["key"], json!("build"));
}

#[test]
fn derived_session_events_capture_sidebar_clears() {
    let mut workspace =
        event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
    workspace.sidebar.progress = Some(json!({"value": 0.5}));
    workspace.sidebar.status_entries =
        sidebar_map(&[("build", json!({"key": "build", "value": "green"}))]);
    workspace.sidebar.metadata_entries =
        sidebar_map(&[("task", json!({"key": "task", "value": "review"}))]);
    workspace.sidebar.metadata_blocks =
        sidebar_map(&[("notes", json!({"key": "notes", "markdown": "Ready"}))]);
    workspace.sidebar.log_entries = vec![json!({"level": "info", "message": "ship it"})];
    let previous = event_summary(vec![workspace], 0);
    let current = event_summary(
        vec![event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a"],
            Some("surface-a"),
        )],
        0,
    );

    let events = derived_session_event_specs(Some(&previous), &current);
    let names = event_names(&events);

    assert!(names.contains(&"sidebar.progress.cleared"));
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "sidebar.metadata.cleared")
            .count(),
        3
    );
    assert!(names.contains(&"sidebar.log.cleared"));
    let cleared = events
        .iter()
        .find(|event| {
            event.name == "sidebar.metadata.cleared"
                && event.payload["kind"] == json!("metadata_block")
        })
        .expect("metadata block cleared event");
    assert_eq!(cleared.payload["key"], json!("notes"));
}

#[test]
fn event_log_append_writes_jsonl_and_rotates_one_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    append_event_line_to_dir(dir.path(), r#"{"seq":1}"#, 64).expect("append first");
    append_event_line_to_dir(dir.path(), r#"{"seq":2}"#, 64).expect("append second");
    assert_eq!(
        fs::read_to_string(dir.path().join(EVENT_LOG_FILE_NAME)).expect("current log"),
        "{\"seq\":1}\n{\"seq\":2}\n"
    );

    append_event_line_to_dir(dir.path(), r#"{"seq":3}"#, 24).expect("rotate append");
    assert_eq!(
        fs::read_to_string(dir.path().join(EVENT_LOG_ARCHIVE_FILE_NAME)).expect("archive log"),
        "{\"seq\":1}\n{\"seq\":2}\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(EVENT_LOG_FILE_NAME)).expect("rotated current log"),
        "{\"seq\":3}\n"
    );
}

#[test]
fn control_socket_methods_advertise_browser_network_and_platform_gaps() {
    for method in [
        "system.capabilities",
        "config.reload",
        "window.list",
        "window.current",
        "notification.list",
        "notification.dismiss",
        "notification.mark_read",
        "notification.clear",
        "notification.open",
        "notification.jump_to_unread",
        "notification.create",
        "right_sidebar",
        "feed.push",
        "feed.list",
        "feed.permission.reply",
        "feed.question.reply",
        "feed.exit_plan.reply",
        "session.restore_previous",
        "events.stream",
        "extension.sidebar.snapshot",
        "sidebar.snapshot",
        "workspace.set_agent_pid",
        "workspace.clear_agent_pid",
        "workspace.move_to_window",
        "workspace.last",
        "surface.report_tty",
        "surface.report_shell_state",
        "surface.split_off",
        "surface.drag_to_split",
        "pane.swap",
        "pane.focus",
        "pane.break",
        "pane.join",
        "pane.last",
        "pane.list",
        "pane.surfaces",
        "pane.resize",
        "surface.move",
        "surface.clear_history",
        "surface.trigger_flash",
        "surface.refresh_all",
        "surface.read_text",
        "workspace.report_pr",
        "workspace.report_review",
        "workspace.clear_pr",
        "workspace.report_meta",
        "workspace.clear_meta",
        "workspace.list_meta",
        "workspace.report_meta_block",
        "workspace.clear_meta_block",
        "workspace.list_meta_blocks",
        "workspace.reset_sidebar",
        "browser.open_split",
        "browser.navigate",
        "browser.reload",
        "browser.url.get",
        "browser.focus_webview",
        "browser.is_webview_focused",
        "browser.snapshot",
        "browser.eval",
        "browser.click",
        "browser.fill",
        "browser.get.text",
        "browser.is.visible",
        "browser.find.role",
        "browser.cookies.get",
        "browser.storage.get",
        "browser.tab.list",
        "browser.console.list",
        "browser.state.save",
        "browser.network.requests",
        "browser.network.clear",
        "browser.viewport.set",
        "browser.geolocation.set",
        "browser.offline.set",
        "browser.trace.start",
        "browser.trace.stop",
        "browser.network.route",
        "browser.network.unroute",
        "browser.screencast.start",
        "browser.screencast.stop",
        "browser.input_mouse",
        "browser.input_keyboard",
        "browser.input_touch",
        "debug.browser.start_direct_proxy",
        "debug.browser.attach_webview",
        "debug.terminals",
    ] {
        assert!(
            CONTROL_SOCKET_METHODS.contains(&method),
            "missing advertised method {method}"
        );
    }
}

#[test]
fn unported_browser_automation_methods_are_explicit_not_supported_contract() {
    assert!(!is_unported_browser_automation_method(
        "browser.network.requests"
    ));
    for method in [
        "browser.snapshot",
        "browser.eval",
        "browser.wait",
        "browser.click",
        "browser.dblclick",
        "browser.hover",
        "browser.focus",
        "browser.type",
        "browser.fill",
        "browser.press",
        "browser.keydown",
        "browser.keyup",
        "browser.check",
        "browser.uncheck",
        "browser.select",
        "browser.scroll",
        "browser.scroll_into_view",
        "browser.screenshot",
        "browser.get.text",
        "browser.get.html",
        "browser.get.value",
        "browser.get.attr",
        "browser.get.title",
        "browser.get.count",
        "browser.get.box",
        "browser.get.styles",
        "browser.is.visible",
        "browser.is.enabled",
        "browser.is.checked",
        "browser.find.role",
        "browser.find.text",
        "browser.find.label",
        "browser.find.placeholder",
        "browser.find.alt",
        "browser.find.title",
        "browser.find.testid",
        "browser.find.first",
        "browser.find.last",
        "browser.find.nth",
        "browser.frame.select",
        "browser.frame.main",
        "browser.dialog.accept",
        "browser.dialog.dismiss",
        "browser.download.wait",
        "browser.cookies.get",
        "browser.cookies.set",
        "browser.cookies.clear",
        "browser.storage.get",
        "browser.storage.set",
        "browser.storage.clear",
        "browser.tab.new",
        "browser.tab.list",
        "browser.tab.switch",
        "browser.tab.close",
        "browser.console.list",
        "browser.console.clear",
        "browser.errors.list",
        "browser.state.save",
        "browser.state.load",
        "browser.highlight",
        "browser.addinitscript",
        "browser.addscript",
        "browser.addstyle",
    ] {
        assert!(
            CONTROL_SOCKET_METHODS.contains(&method),
            "missing implemented browser automation method {method}"
        );
        assert!(
            !is_unported_browser_automation_method(method),
            "implemented browser automation method should not route to not_supported: {method}"
        );
    }
    assert!(!is_unported_browser_automation_method(
        "browser.viewport.set"
    ));
}

#[test]
fn browser_surface_payload_returns_agent_browser_shape() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".to_string()),
            panel_ids: vec!["surface-1".to_string()],
            selected_panel_id: Some("surface-1".to_string()),
            surface_kind: Some("browser".to_string()),
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: Some("https://example.com/path".to_string()),
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));

    let payload = browser_surface_payload(&snapshot, 0, "surface-1")
        .expect("browser surface payload should exist");

    assert_eq!(payload["surface_id"], json!("surface-1"));
    assert_eq!(payload["panel_id"], json!("surface-1"));
    assert_eq!(payload["surface_ref"], json!("surface:1"));
    assert_eq!(payload["workspace_ref"], json!("workspace:1"));
    assert_eq!(payload["url"], json!("https://example.com/path"));
    assert_eq!(payload["surface"]["type"], json!("browser"));
}

#[test]
fn new_browser_surface_id_prefers_new_browser_surface() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Split(
        cmux_core::session::SessionSplitLayoutSnapshot {
            split_id: None,
            orientation: SessionSplitOrientation::Horizontal,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                SessionPaneLayoutSnapshot {
                    pane_id: Some("pane-1".to_string()),
                    panel_ids: vec!["surface-1".to_string()],
                    selected_panel_id: Some("surface-1".to_string()),
                    surface_kind: None,
                    markdown_file_path: None,
                    file_path: None,
                    diff_viewer_token: None,
                    diff_viewer_request_path: None,
                    browser_url: None,
                    browser_proxy_url: None,
                    browser_back_history: None,
                    browser_forward_history: None,
                    browser_omnibar_visible: None,
                    browser_focus_mode_active: None,
                    browser_developer_tools_visible: None,
                    browser_developer_tools_panel: None,
                    browser_page_zoom: None,
                },
            )),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                SessionPaneLayoutSnapshot {
                    pane_id: Some("pane-2".to_string()),
                    panel_ids: vec!["surface-2".to_string()],
                    selected_panel_id: Some("surface-2".to_string()),
                    surface_kind: Some("browser".to_string()),
                    markdown_file_path: None,
                    file_path: None,
                    diff_viewer_token: None,
                    diff_viewer_request_path: None,
                    browser_url: Some("about:blank".to_string()),
                    browser_proxy_url: None,
                    browser_back_history: None,
                    browser_forward_history: None,
                    browser_omnibar_visible: None,
                    browser_focus_mode_active: None,
                    browser_developer_tools_visible: None,
                    browser_developer_tools_panel: None,
                    browser_page_zoom: None,
                },
            )),
            divider_position: 0.5,
        },
    ));

    assert_eq!(
        new_browser_surface_id(&snapshot, 0, &["surface-1".to_string()]).as_deref(),
        Some("surface-2")
    );
}

#[test]
fn workspace_list_payload_includes_disconnected_remote_default() {
    let payload = workspace_list_payload(&test_snapshot());
    let remote = &payload["workspaces"][0]["remote"];
    assert_eq!(remote["enabled"], json!(false));
    assert_eq!(remote["state"], json!("disconnected"));
    assert_eq!(remote["connected"], json!(false));
    assert_eq!(remote["active_terminal_sessions"], json!(0));
    assert_eq!(remote["proxy"]["state"], json!("unavailable"));
    assert_eq!(remote["proxy"]["url"], Value::Null);
    assert_eq!(remote["daemon"]["state"], json!("unavailable"));
}

#[test]
fn workspace_list_payload_includes_remote_proxy_endpoint() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: "connected".to_string(),
        connected: true,
        transport: Some("ssh".to_string()),
        destination: Some("dev.example.com".to_string()),
        port: Some(22),
        local_proxy_port: Some(31337),
        persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
        has_ssh_options: true,
        detail: None,
        daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
            state: "ready".to_string(),
            capabilities: vec!["proxy.stream.push".to_string()],
        }),
        proxy: Some(SessionWorkspaceRemoteProxySnapshot {
            state: "ready".to_string(),
            host: Some("127.0.0.1".to_string()),
            port: Some(31337),
            schemes: vec!["socks5".to_string(), "http_connect".to_string()],
            url: Some("socks5://127.0.0.1:31337".to_string()),
            error_code: None,
        }),
        detected_ports: Vec::new(),
        forwarded_ports: Vec::new(),
        conflicted_ports: Vec::new(),
        active_terminal_sessions: Some(1),
    });

    let payload = workspace_list_payload(&snapshot);
    let remote = &payload["workspaces"][0]["remote"];
    assert_eq!(remote["enabled"], json!(true));
    assert_eq!(remote["state"], json!("connected"));
    assert_eq!(remote["destination"], json!("dev.example.com"));
    assert_eq!(remote["local_proxy_port"], json!(31337));
    assert_eq!(remote["proxy"]["url"], json!("socks5://127.0.0.1:31337"));
    assert_eq!(
        remote["daemon"]["capabilities"],
        json!(["proxy.stream.push"])
    );
}

#[test]
fn workspace_list_payload_includes_workspace_runtime_metadata() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.initial_terminal_command = Some("npm run dev".to_string());
    workspace.initial_terminal_input = Some("ready".to_string());
    workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
        "NODE_ENV".to_string(),
        "development".to_string(),
    )]));
    workspace.zoomed_panel_id = Some("surface-1".to_string());
    workspace.restorable_agent_snapshots = Some(vec![SessionPanelRestorableAgentSnapshot {
        panel_id: "surface-1".to_string(),
        snapshot: SessionRestorableAgentSnapshot {
            kind: "codex".to_string(),
            session_id: "session-1".to_string(),
            working_directory: Some("C:/repo".to_string()),
            launch_command: None,
            resume_command: Some("codex resume session-1".to_string()),
            fork_command: Some("codex fork session-1".to_string()),
        },
    }]);
    workspace.listening_ports = Some(vec![5173]);
    workspace.agent_listening_ports = Some(vec![4173]);
    workspace.agent_pids = Some(vec![SessionWorkspaceAgentPidSnapshot {
        key: "codex.session-1".to_string(),
        pid: 1234,
        updated_at: 20,
    }]);
    workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "surface-1".to_string(),
        ports: vec![3000, 5173],
    }]);
    workspace.panel_ttys = Some(vec![SessionPanelTtySnapshot {
        panel_id: "surface-1".to_string(),
        tty: "ttys004".to_string(),
        updated_at: 21,
    }]);
    workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
        panel_id: "surface-1".to_string(),
        state: SessionPanelShellActivityStateSnapshot::CommandRunning,
        updated_at: 22,
    }]);

    let payload = workspace_list_payload(&snapshot);
    let summary = &payload["workspaces"][0];
    assert_eq!(summary["initial_terminal_command"], json!("npm run dev"));
    assert_eq!(summary["initial_terminal_input"], json!("ready"));
    assert_eq!(
        summary["initial_terminal_environment"],
        json!({"NODE_ENV": "development"})
    );
    assert_eq!(summary["zoomed_panel_id"], json!("surface-1"));
    assert_eq!(
        summary["restorable_agent_panels"],
        json!([{
            "panel_id": "surface-1",
            "kind": "codex",
            "session_id": "session-1",
            "working_directory": "C:/repo",
            "resume_command": "codex resume session-1",
            "fork_command": "codex fork session-1",
        }])
    );
    assert_eq!(summary["listening_ports"], json!([3000, 4173, 5173]));
    assert_eq!(summary["agent_listening_ports"], json!([4173]));
    assert_eq!(
        summary["agent_pids"],
        json!([{"key": "codex.session-1", "pid": 1234, "updated_at": 20}])
    );
    assert_eq!(
        summary["panel_ttys"],
        json!([{"panel_id": "surface-1", "tty": "ttys004", "updated_at": 21}])
    );
    assert_eq!(
        summary["panel_shell_activity"],
        json!([{"panel_id": "surface-1", "state": "commandRunning", "updated_at": 22}])
    );
}

#[test]
fn workspace_list_payload_includes_workspace_group_metadata() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    second.group_id = Some("group-1".to_string());
    snapshot.windows[0].tab_manager.workspaces[0].group_id = Some("group-1".to_string());
    snapshot.windows[0].tab_manager.workspace_groups = Some(vec![SessionWorkspaceGroupSnapshot {
        id: "group-1".to_string(),
        name: "Backend".to_string(),
        is_collapsed: true,
        anchor_workspace_id: Some("workspace-1".to_string()),
        anchor_member_index: Some(0),
        is_pinned: Some(true),
        custom_color: Some("#123456".to_string()),
        icon_symbol: Some("folder".to_string()),
    }]);
    snapshot.windows[0].tab_manager.workspaces.push(second);

    let payload = workspace_list_payload(&snapshot);
    assert_eq!(payload["workspaces"][0]["group_id"], json!("group-1"));
    assert_eq!(payload["workspace_groups"][0]["id"], json!("group-1"));
    assert_eq!(payload["workspace_groups"][0]["name"], json!("Backend"));
    assert_eq!(payload["workspace_groups"][0]["collapsed"], json!(true));
    assert_eq!(payload["workspace_groups"][0]["pinned"], json!(true));
    assert_eq!(
        payload["workspace_groups"][0]["anchor_workspace_id"],
        json!("workspace-1")
    );
    assert_eq!(
        payload["workspace_groups"][0]["members"],
        json!([
            {"workspace_id": "workspace-1", "workspace_ref": "workspace:1"},
            {"workspace_id": "workspace-2", "workspace_ref": "workspace:2"},
        ])
    );
    assert!(payload["workspaces"][0].get("index").is_none());
    assert!(payload["workspace_groups"][0]["members"][0]
        .get("index")
        .is_none());
}

#[test]
fn surface_list_payload_projects_pane_surfaces() {
    let result = surface_list(&test_snapshot());
    let ControlCallResult::Ok(value) = result else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    assert_eq!(payload["workspace_id"], json!("workspace-1"));
    assert_eq!(payload["surfaces"][0]["id"], json!("surface-1"));
    assert_eq!(payload["surfaces"][0]["ref"], json!("surface:1"));
    assert!(payload["surfaces"][0].get("index").is_none());
    assert!(payload["surfaces"][0].get("index_in_pane").is_none());
    assert_eq!(payload["surfaces"][0]["type"], json!("terminal"));
    assert_eq!(payload["surfaces"][0]["pane_id"], json!("pane-1"));
    assert_eq!(payload["surfaces"][0]["custom_title"], Value::Null);
    assert_eq!(payload["surfaces"][0]["pinned"], json!(false));
    assert_eq!(payload["surfaces"][0]["unread"], json!(false));
    assert_eq!(
        payload["surfaces"][0]["requested_working_directory"],
        json!("C:/repo")
    );
    assert_eq!(payload["surfaces"][0]["initial_command"], Value::Null);
    assert_eq!(payload["surfaces"][0]["initial_input"], Value::Null);
    assert_eq!(payload["surfaces"][0]["initial_environment"], Value::Null);
    assert_eq!(payload["surfaces"][0]["listening_ports"], json!([]));
    assert_eq!(payload["surfaces"][0]["markdown_file_path"], Value::Null);
    assert_eq!(payload["surfaces"][0]["diff_viewer_token"], Value::Null);
    assert_eq!(
        payload["surfaces"][0]["diff_viewer_request_path"],
        Value::Null
    );
    assert_eq!(payload["surfaces"][0]["browser_url"], Value::Null);
    assert_eq!(payload["surfaces"][0]["browser_can_go_back"], json!(false));
    assert_eq!(
        payload["surfaces"][0]["browser_omnibar_visible"],
        json!(true)
    );
    assert_eq!(
        payload["surfaces"][0]["browser_developer_tools_visible"],
        json!(false)
    );
}

#[test]
fn workspace_current_workspace_selector_routes_manager_and_returns_selection() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

    let ControlCallResult::Ok(value) = workspace_current_from_params(
        &snapshot,
        &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"))]),
    ) else {
        panic!("workspace current should succeed");
    };
    let payload: Value = value.into();
    assert_eq!(payload["workspace_id"], json!("workspace-1"));
    assert_eq!(payload["workspace_ref"], json!("workspace:1"));
    assert_eq!(payload["workspace"]["selected"], json!(true));
}

#[test]
fn surface_list_can_be_scoped_to_background_workspace() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-2".to_string()),
            panel_ids: vec!["surface-2".to_string()],
            selected_panel_id: Some("surface-2".to_string()),
            surface_kind: Some("browser".to_string()),
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: Some("https://background.test".to_string()),
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

    let ControlCallResult::Ok(value) = surface_list_from_params(
        &snapshot,
        &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"))]),
    ) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    assert_eq!(payload["workspace_id"], json!("workspace-2"));
    assert_eq!(payload["workspace_ref"], json!("workspace:2"));
    assert_eq!(payload["surfaces"][0]["id"], json!("surface-2"));
    assert_eq!(
        payload["surfaces"][0]["browser_url"],
        json!("https://background.test")
    );
}

#[test]
fn surface_list_payload_inherits_workspace_terminal_startup() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.initial_terminal_command = Some("cargo test".to_string());
    workspace.initial_terminal_input = Some("echo ready".to_string());
    workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
        "RUST_LOG".to_string(),
        "debug".to_string(),
    )]));

    let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    let surface = &payload["surfaces"][0];
    assert_eq!(surface["requested_working_directory"], json!("C:/repo"));
    assert_eq!(surface["initial_command"], json!("cargo test"));
    assert_eq!(surface["initial_input"], json!("echo ready"));
    assert_eq!(surface["initial_environment"], json!({"RUST_LOG": "debug"}));
}

#[test]
fn surface_list_payload_includes_surface_metadata() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.initial_terminal_command = Some("cargo test".to_string());
    workspace.initial_terminal_input = Some("workspace input".to_string());
    workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
        "WORKSPACE".to_string(),
        "1".to_string(),
    )]));
    workspace.panel_titles = Some(vec![SessionPanelTitleSnapshot {
        panel_id: "surface-1".to_string(),
        custom_title: Some("API logs".to_string()),
    }]);
    workspace.panel_pins = Some(vec![SessionPanelPinSnapshot {
        panel_id: "surface-1".to_string(),
        is_pinned: true,
    }]);
    workspace.panel_unreads = Some(vec![SessionPanelUnreadSnapshot {
        panel_id: "surface-1".to_string(),
        is_unread: true,
        unread_at: None,
    }]);
    workspace.panel_terminal_startups = Some(vec![SessionPanelTerminalStartupSnapshot {
        panel_id: "surface-1".to_string(),
        initial_terminal_command: Some("npm test".to_string()),
        initial_terminal_input: Some("hello".to_string()),
        initial_terminal_environment: Some(BTreeMap::from_iter([(
            "CI".to_string(),
            "1".to_string(),
        )])),
    }]);
    workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "surface-1".to_string(),
        ports: vec![8080, 3000],
    }]);
    workspace.panel_ttys = Some(vec![SessionPanelTtySnapshot {
        panel_id: "surface-1".to_string(),
        tty: "/dev/pts/7".to_string(),
        updated_at: 22,
    }]);
    workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
        panel_id: "surface-1".to_string(),
        state: SessionPanelShellActivityStateSnapshot::PromptIdle,
        updated_at: 23,
    }]);

    let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    let surface = &payload["surfaces"][0];
    assert_eq!(surface["title"], json!("API logs"));
    assert_eq!(surface["custom_title"], json!("API logs"));
    assert_eq!(surface["pinned"], json!(true));
    assert_eq!(surface["unread"], json!(true));
    assert_eq!(surface["requested_working_directory"], json!("C:/repo"));
    assert_eq!(surface["initial_command"], json!("npm test"));
    assert_eq!(surface["initial_input"], json!("hello"));
    assert_eq!(surface["initial_environment"], json!({"CI": "1"}));
    assert_eq!(surface["listening_ports"], json!([3000, 8080]));
    assert_eq!(surface["tty"], json!("/dev/pts/7"));
    assert_eq!(surface["tty_name"], json!("/dev/pts/7"));
    assert_eq!(surface["shell_activity"], json!("promptIdle"));
    assert_eq!(surface["shell_activity_state"], json!("promptIdle"));
}

#[test]
fn debug_terminals_payload_includes_reported_tty() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].panel_ttys =
        Some(vec![SessionPanelTtySnapshot {
            panel_id: "surface-1".to_string(),
            tty: "ttys004".to_string(),
            updated_at: 23,
        }]);

    let mut terminals = Vec::new();
    let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
    for (surface_index, surface) in surfaces_for_workspace(workspace).into_iter().enumerate() {
        terminals.push(json!({
            "workspace_id": workspace.workspace_id,
            "workspace_ref": workspace_ref(0),
            "surface_id": surface.get("id").cloned().unwrap_or(Value::Null),
            "surface_ref": surface_ref(surface_index),
            "tty": surface.get("tty").cloned().unwrap_or(Value::Null),
        }));
    }

    assert_eq!(
        terminals[0],
        json!({
            "workspace_id": "workspace-1",
            "workspace_ref": "workspace:1",
            "surface_id": "surface-1",
            "surface_ref": "surface:1",
            "tty": "ttys004",
        })
    );
}

#[test]
fn surface_list_payload_includes_restorable_agent_binding() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.restorable_agent_snapshots = Some(vec![SessionPanelRestorableAgentSnapshot {
        panel_id: "surface-1".to_string(),
        snapshot: SessionRestorableAgentSnapshot {
            kind: "codex".to_string(),
            session_id: "session-1".to_string(),
            working_directory: Some("C:/repo".to_string()),
            launch_command: Some(AgentLaunchCommandSnapshot {
                launcher: None,
                executable_path: Some("codex".to_string()),
                arguments: vec!["resume".to_string(), "session-1".to_string()],
                working_directory: Some("C:/repo".to_string()),
                environment: Some(BTreeMap::from_iter([(
                    "CODEX_HOME".to_string(),
                    "C:/codex".to_string(),
                )])),
                source: Some("provider.start".to_string()),
            }),
            resume_command: Some("codex resume session-1".to_string()),
            fork_command: Some("codex fork session-1".to_string()),
        },
    }]);

    let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    let binding = &payload["surfaces"][0]["resume_binding"];
    assert_eq!(binding["kind"], json!("codex"));
    assert_eq!(binding["session_id"], json!("session-1"));
    assert_eq!(binding["working_directory"], json!("C:/repo"));
    assert_eq!(binding["resume_command"], json!("codex resume session-1"));
    assert_eq!(binding["fork_command"], json!("codex fork session-1"));
    assert_eq!(binding["launch_command"]["executable_path"], json!("codex"));
    assert_eq!(
        binding["launch_command"]["environment"],
        json!({"CODEX_HOME": "C:/codex"})
    );
}

#[test]
fn surface_list_payload_includes_markdown_file_and_diff_state() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".to_string()),
            panel_ids: vec!["surface-1".to_string()],
            selected_panel_id: Some("surface-1".to_string()),
            surface_kind: Some("diff".to_string()),
            markdown_file_path: Some("C:/repo/README.md".to_string()),
            file_path: Some("C:/repo/notes.txt".to_string()),
            diff_viewer_token: Some("tok-abcdef0123456789".to_string()),
            diff_viewer_request_path: Some("/review/index.html".to_string()),
            browser_url: None,
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));
    let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    let surface = &payload["surfaces"][0];
    assert_eq!(surface["type"], json!("diff"));
    assert_eq!(surface["markdown_file_path"], json!("C:/repo/README.md"));
    assert_eq!(surface["file_path"], json!("C:/repo/notes.txt"));
    assert_eq!(surface["diff_viewer_token"], json!("tok-abcdef0123456789"));
    assert_eq!(
        surface["diff_viewer_request_path"],
        json!("/review/index.html")
    );
}

#[test]
fn surface_list_payload_includes_browser_state() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".to_string()),
            panel_ids: vec!["surface-1".to_string()],
            selected_panel_id: Some("surface-1".to_string()),
            surface_kind: Some("browser".to_string()),
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: Some("https://example.com".to_string()),
            browser_proxy_url: Some("socks5://127.0.0.1:31337".to_string()),
            browser_back_history: Some(vec!["https://previous.test".to_string()]),
            browser_forward_history: Some(vec!["https://forward.test".to_string()]),
            browser_omnibar_visible: Some(false),
            browser_focus_mode_active: Some(true),
            browser_developer_tools_visible: Some(true),
            browser_developer_tools_panel: Some("console".to_string()),
            browser_page_zoom: Some(1.25),
        },
    ));
    let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
        panic!("surface list should succeed");
    };
    let payload: Value = value.into();
    let surface = &payload["surfaces"][0];
    assert_eq!(surface["type"], json!("browser"));
    assert_eq!(surface["browser_url"], json!("https://example.com"));
    assert_eq!(
        surface["browser_proxy_url"],
        json!("socks5://127.0.0.1:31337")
    );
    assert_eq!(surface["browser_can_go_back"], json!(true));
    assert_eq!(surface["browser_can_go_forward"], json!(true));
    assert_eq!(surface["browser_back_history_count"], json!(1));
    assert_eq!(surface["browser_forward_history_count"], json!(1));
    assert_eq!(surface["browser_omnibar_visible"], json!(false));
    assert_eq!(surface["browser_focus_mode_active"], json!(true));
    assert_eq!(surface["browser_developer_tools_visible"], json!(true));
    assert_eq!(surface["browser_developer_tools_panel"], json!("console"));
    assert_eq!(surface["browser_page_zoom"], json!(1.25));
}

#[test]
fn ports_param_accepts_array_string_and_single_port() {
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "ports".to_string(),
            json!([3000, "5173", 3000]),
        )])),
        Some(vec![3000, 5173])
    );
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "listening_ports".to_string(),
            json!("8080, 9000 8080"),
        )])),
        Some(vec![8080, 9000])
    );
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "port".to_string(),
            json!(1)
        )])),
        Some(vec![1])
    );
}

#[test]
fn ports_param_rejects_out_of_range_or_non_integer_ports() {
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "ports".to_string(),
            json!([0])
        )])),
        None
    );
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "ports".to_string(),
            json!([65536]),
        )])),
        None
    );
    assert_eq!(
        ports_param(&serde_json::Map::from_iter([(
            "ports".to_string(),
            json!(["abc"]),
        )])),
        None
    );
}

#[test]
fn optional_u16_param_accepts_numbers_strings_and_null_clear() {
    assert_eq!(
        optional_u16_param(
            &serde_json::Map::from_iter([("local_proxy_port".to_string(), json!(31337))]),
            "local_proxy_port",
        ),
        Some(Some(31337))
    );
    assert_eq!(
        optional_u16_param(
            &serde_json::Map::from_iter([("local_proxy_port".to_string(), json!("31338"))]),
            "local_proxy_port",
        ),
        Some(Some(31338))
    );
    assert_eq!(
        optional_u16_param(
            &serde_json::Map::from_iter([("local_proxy_port".to_string(), Value::Null)]),
            "local_proxy_port",
        ),
        Some(None)
    );
    assert_eq!(
        optional_u16_param(&serde_json::Map::new(), "local_proxy_port"),
        Some(None)
    );
}

#[test]
fn optional_u16_param_rejects_invalid_ports() {
    for value in [json!(0), json!(65536), json!("abc"), json!([31337])] {
        assert_eq!(
            optional_u16_param(
                &serde_json::Map::from_iter([("local_proxy_port".to_string(), value)]),
                "local_proxy_port",
            ),
            None
        );
    }
}

#[test]
fn string_vec_param_accepts_arrays_and_newline_strings() {
    assert_eq!(
        string_vec_param(
            &serde_json::Map::from_iter([(
                "ssh_options".to_string(),
                json!([
                    "StrictHostKeyChecking=no",
                    " ",
                    42,
                    "UserKnownHostsFile=/dev/null"
                ]),
            )]),
            &["ssh_options"],
        ),
        Some(vec![
            "StrictHostKeyChecking=no".to_string(),
            "UserKnownHostsFile=/dev/null".to_string(),
        ])
    );
    assert_eq!(
        string_vec_param(
            &serde_json::Map::from_iter([(
                "sshOptions".to_string(),
                json!("ControlMaster=auto\n\nControlPersist=600"),
            )]),
            &["ssh_options", "sshOptions"],
        ),
        Some(vec![
            "ControlMaster=auto".to_string(),
            "ControlPersist=600".to_string(),
        ])
    );
}

#[test]
fn surface_ports_kick_target_accepts_known_surface() {
    let target = surface_ports_kick_target(
        &test_snapshot(),
        &serde_json::Map::from_iter([("surface_id".to_string(), json!("surface-1"))]),
    )
    .expect("ports kick should target a known surface");
    assert_eq!(target, (0, "surface-1".to_string()));
}

#[test]
fn surface_ports_kick_target_uses_workspace_scope_with_surface_index() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-2".to_string()),
            panel_ids: vec!["surface-2".to_string(), "surface-3".to_string()],
            selected_panel_id: Some("surface-3".to_string()),
            surface_kind: None,
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: None,
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

    let target = surface_ports_kick_target(
        &snapshot,
        &serde_json::Map::from_iter([
            ("workspace_id".to_string(), json!("workspace-2")),
            ("surface_ref".to_string(), json!("surface:1")),
        ]),
    )
    .expect("ports kick should target scoped surface ref");
    assert_eq!(target, (1, "surface-2".to_string()));
}

#[test]
fn workspace_index_from_params_accepts_refs_or_workspace_id_not_index() {
    let snapshot = test_snapshot();
    assert_eq!(
        workspace_index_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
        ),
        None
    );
    assert_eq!(
        workspace_index_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-1"),)])
        ),
        Some(0)
    );
    assert_eq!(
        workspace_index_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_ref".to_string(), json!("workspace:1"),)])
        ),
        Some(0)
    );
    assert_eq!(
        workspace_index_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("index".to_string(), json!(99),)])
        ),
        None
    );
}

#[test]
fn workspace_indices_from_params_accepts_bulk_refs_and_ids_not_indices() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    snapshot.windows[0].tab_manager.workspaces.push(second);

    assert_eq!(
        workspace_indices_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("indices".to_string(), json!([0, "1"]),)])
        ),
        None
    );
    assert_eq!(
        workspace_indices_from_params(
            &snapshot,
            &serde_json::Map::from_iter([
                ("workspace_refs".to_string(), json!(["workspace:2"])),
                ("workspace_ids".to_string(), json!(["workspace-1"])),
            ])
        ),
        Some(vec![1, 0])
    );
    assert_eq!(
        workspace_indices_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_refs".to_string(), json!(["workspace:3"]),)])
        ),
        None
    );
}

#[test]
fn workspace_reorder_destination_accepts_exactly_one_canonical_target() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    let mut third = snapshot.windows[0].tab_manager.workspaces[0].clone();
    third.workspace_id = Some("workspace-3".to_string());
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.workspaces.push(third);

    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([(
                "before_workspace_ref".to_string(),
                json!("workspace:3"),
            )]),
            0,
        ),
        Some(1)
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([(
                "after_workspace_ref".to_string(),
                json!("workspace:3"),
            )]),
            0,
        ),
        Some(2)
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([(
                "before_workspace_id".to_string(),
                json!("workspace-1"),
            )]),
            2,
        ),
        Some(0)
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([
                ("after_workspace_id".to_string(), json!("workspace-1"),)
            ]),
            2,
        ),
        Some(1)
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([("index".to_string(), json!(0))]),
            2,
        ),
        Some(0)
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([
                ("index".to_string(), json!(0)),
                ("before_workspace_ref".to_string(), json!("workspace:2"),),
            ]),
            2,
        ),
        None
    );
    assert_eq!(
        workspace_reorder_destination_index(
            &snapshot,
            &serde_json::Map::from_iter([("to_index".to_string(), json!(0))]),
            2,
        ),
        None
    );
}

#[test]
fn workspace_reorder_window_scope_cannot_fall_through_to_first_window() {
    let snapshot = test_snapshot();
    assert!(workspace_reorder_window_matches(
        &snapshot,
        &serde_json::Map::new()
    ));
    assert!(workspace_reorder_window_matches(
        &snapshot,
        &serde_json::Map::from_iter([("window_ref".to_string(), json!("window:1"),)])
    ));
    assert!(!workspace_reorder_window_matches(
        &snapshot,
        &serde_json::Map::from_iter([("window_ref".to_string(), json!("window:2"),)])
    ));
}

#[test]
fn workspace_reorder_many_order_resolves_refs_and_ids_in_request_order() {
    let first = "00000000-0000-0000-0000-000000000001";
    let second = "00000000-0000-0000-0000-000000000002";
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].workspace_id = Some(first.to_string());
    let mut workspace = snapshot.windows[0].tab_manager.workspaces[0].clone();
    workspace.workspace_id = Some(second.to_string());
    snapshot.windows[0].tab_manager.workspaces.push(workspace);

    let order = workspace_reorder_many_order(
        &snapshot,
        &serde_json::Map::from_iter([("workspace_ids".to_string(), json!(["workspace:2", first]))]),
    )
    .unwrap();
    assert_eq!(
        order,
        [
            Uuid::parse_str(second).unwrap(),
            Uuid::parse_str(first).unwrap()
        ]
    );
    assert!(matches!(
        workspace_reorder_many_order(&snapshot, &serde_json::Map::new()),
        Err(WorkspaceReorderManyOrderError::Missing)
    ));
}

#[test]
fn workspace_index_defaults_to_selected_for_current_commands() {
    let snapshot = test_snapshot();
    assert_eq!(
        workspace_index_from_params_or_selected(&snapshot, &serde_json::Map::new()),
        Some(0)
    );
}

#[test]
fn workspace_index_from_params_requires_explicit_selector_for_close_commands() {
    let snapshot = test_snapshot();
    assert_eq!(
        canonical_workspace_target_index(&snapshot, 0, &serde_json::Map::new()),
        None
    );
    assert_eq!(
        canonical_workspace_target_index(
            &snapshot,
            0,
            &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
        ),
        None
    );
    assert_eq!(
        canonical_workspace_target_index(
            &snapshot,
            0,
            &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-1"),)])
        ),
        Some(0)
    );
    assert_eq!(
        canonical_workspace_target_index(
            &snapshot,
            0,
            &serde_json::Map::from_iter([("workspace_ref".to_string(), json!("workspace:1"),)])
        ),
        None
    );
}

#[test]
fn workspace_v2_list_has_canonical_shape_only() {
    let payload = workspace_list_payload_for_window(&test_snapshot(), 0);
    let object = payload.as_object().expect("workspace.list object");
    assert_eq!(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["window_id", "window_ref", "workspaces"])
    );
    let row = object["workspaces"][0]
        .as_object()
        .expect("workspace summary object");
    assert_eq!(
        row.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "id",
            "ref",
            "title",
            "custom_title",
            "has_custom_title",
            "description",
            "selected",
            "pinned",
            "listening_ports",
            "remote",
            "current_directory",
            "custom_color",
            "latest_conversation_message",
            "latest_submitted_message",
            "latest_submitted_at",
            "index",
        ])
    );
}

#[test]
fn workspace_v2_current_routes_by_workspace_but_returns_owner_selection() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].window_id = Some("window-a".to_string());
    let mut background = snapshot.windows[0].clone();
    background.window_id = Some("window-b".to_string());
    background.tab_manager.workspaces[0].workspace_id = Some("workspace-b1".to_string());
    let mut requested = background.tab_manager.workspaces[0].clone();
    requested.workspace_id = Some("workspace-b2".to_string());
    background.tab_manager.workspaces.push(requested);
    background.tab_manager.selected_workspace_index = Some(0);
    snapshot.windows.push(background);

    let params = serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-b2"))]);
    let ControlCallResult::Ok(result) = workspace_current_from_params(&snapshot, &params) else {
        panic!("workspace.current should resolve the owning window");
    };
    let result: Value = result.into();
    assert_eq!(result["window_id"], json!("window-b"));
    assert_eq!(result["workspace_id"], json!("workspace-b1"));
    assert_eq!(result["workspace"]["selected"], json!(true));
}

#[test]
fn workspace_v2_current_invalid_explicit_window_never_falls_back() {
    let params = serde_json::Map::from_iter([
        ("window_id".to_string(), json!("missing-window")),
        ("workspace_id".to_string(), json!("workspace-1")),
    ]);
    let result = workspace_current_from_params(&test_snapshot(), &params);
    assert!(matches!(
        result,
        ControlCallResult::Err { code, message, .. }
            if code == "unavailable" && message == "TabManager not available"
    ));
}

#[test]
fn workspace_v2_current_stale_selection_preserves_identity_with_null_summary() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].selected_workspace_id = Some("workspace-1".to_string());
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(99);

    let ControlCallResult::Ok(result) =
        workspace_current_from_params(&snapshot, &serde_json::Map::new())
    else {
        panic!("stale selected identity must still produce workspace.current success");
    };
    let result: Value = result.into();
    assert_eq!(result["workspace_id"], json!("workspace-1"));
    assert!(result["workspace_ref"].as_str().is_some());
    assert_eq!(result["workspace"], Value::Null);
}

#[test]
fn workspace_v2_null_window_selector_falls_through_to_resolvable_workspace() {
    let params = serde_json::Map::from_iter([
        ("window_id".to_string(), Value::Null),
        ("workspace_id".to_string(), json!("workspace-1")),
    ]);
    assert_eq!(
        workspace_routed_window_index(&test_snapshot(), &params),
        Some(0)
    );
}

#[test]
fn workspace_v2_routing_uses_canonical_selector_precedence() {
    let mut snapshot = test_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0].group_id = Some("group-a".to_string());
    let mut background = snapshot.windows[0].clone();
    background.window_id = Some("window-b".to_string());
    background.tab_manager.workspaces[0].workspace_id = Some("workspace-b".to_string());
    background.tab_manager.workspaces[0].group_id = Some("group-b".to_string());
    if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        background.tab_manager.workspaces[0].layout.as_mut()
    {
        pane.pane_id = Some("pane-b".to_string());
        pane.panel_ids = vec!["surface-b".to_string()];
        pane.selected_panel_id = Some("surface-b".to_string());
    }
    snapshot.windows.push(background);

    assert_eq!(
        workspace_routed_window_index_with_active_window(
            &snapshot,
            &serde_json::Map::new(),
            Some("window-b"),
        ),
        Some(1),
        "selectorless routing follows the active session window"
    );
    assert_eq!(
        workspace_routed_window_index(
            &snapshot,
            &serde_json::Map::from_iter([
                ("group_id".to_string(), json!("group-b")),
                ("workspace_id".to_string(), json!("workspace-1")),
            ]),
        ),
        Some(1)
    );
    for key in [
        "workspace_id",
        "surface_id",
        "terminal_id",
        "tab_id",
        "pane_id",
    ] {
        let value = match key {
            "workspace_id" => "workspace-b",
            "pane_id" => "pane-b",
            _ => "surface-b",
        };
        assert_eq!(
            workspace_routed_window_index(
                &snapshot,
                &serde_json::Map::from_iter([(key.to_string(), json!(value))]),
            ),
            Some(1),
            "selector {key}"
        );
    }
}

#[test]
fn workspace_v2_not_found_mints_workspace_ref_for_uuid_identity() {
    let workspace_id = "00000000-0000-0000-0000-000000000099";
    let mut registry = ControlHandleRegistry::default();
    let reference = registry.mint("workspace", workspace_id);
    let result = workspace_not_found_with_ref(workspace_id, &reference);
    let ControlCallResult::Err {
        data: Some(data), ..
    } = result
    else {
        panic!("not_found should include identity data");
    };
    let data: Value = data.into();
    assert_eq!(data["workspace_id"], json!(workspace_id));
    assert!(data["workspace_ref"].as_str().is_some());
}

#[test]
fn workspace_handle_registry_is_stable_and_resolves_by_kind() {
    let mut registry = ControlHandleRegistry::default();
    let first = registry.mint("workspace", "workspace-a");
    let repeated = registry.mint("workspace", "workspace-a");
    let window = registry.mint("window", "window-a");

    assert_eq!(first, repeated);
    assert_eq!(
        registry.resolve("workspace", &first).as_deref(),
        Some("workspace-a")
    );
    assert_eq!(
        registry.resolve("window", &window).as_deref(),
        Some("window-a")
    );
    assert_eq!(registry.resolve("window", &first), None);
}

#[test]
fn workspace_group_placement_uses_group_boundaries_and_reference() {
    let mut snapshot = test_snapshot();
    let first = &mut snapshot.windows[0].tab_manager.workspaces[0];
    first.group_id = Some("group-a".to_string());
    let mut second = first.clone();
    second.workspace_id = Some("workspace-2".to_string());
    let mut ungrouped = first.clone();
    ungrouped.workspace_id = Some("workspace-3".to_string());
    ungrouped.group_id = None;
    let tabs = &mut snapshot.windows[0].tab_manager;
    tabs.workspaces.extend([second, ungrouped]);

    assert_eq!(
        workspace_group_insert_index(tabs, "group-a", "top", None),
        Some(1)
    );
    assert_eq!(
        workspace_group_insert_index(tabs, "group-a", "end", None),
        Some(2)
    );
    assert_eq!(
        workspace_group_insert_index(tabs, "group-a", "afterCurrent", Some(0)),
        Some(1)
    );
}

#[test]
fn workspace_group_create_cwd_prefers_explicit_then_first_eligible_child() {
    let mut snapshot = test_snapshot();
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let anchor_id = Uuid::new_v4();
    let first = &mut snapshot.windows[0].tab_manager.workspaces[0];
    first.workspace_id = Some(first_id.to_string());
    first.current_directory = Some("C:/first".to_string());
    first.is_pinned = Some(true);
    let mut second = first.clone();
    second.workspace_id = Some(second_id.to_string());
    second.current_directory = Some("C:/second".to_string());
    second.is_pinned = None;
    snapshot.windows[0].tab_manager.workspaces.push(second);

    assert_eq!(
        workspace_group_create_cwd(
            &snapshot.windows[0].tab_manager,
            None,
            &[first_id, second_id],
            &HashSet::new(),
        )
        .as_deref(),
        Some("C:/second")
    );
    assert_eq!(
        workspace_group_create_cwd(
            &snapshot.windows[0].tab_manager,
            Some("  explicit  ".to_string()),
            &[second_id],
            &HashSet::from([anchor_id]),
        )
        .as_deref(),
        Some("explicit")
    );
    assert_eq!(
        workspace_group_create_cwd(
            &snapshot.windows[0].tab_manager,
            Some(" file:///C:/repo/worktree ".to_string()),
            &[second_id],
            &HashSet::new(),
        )
        .as_deref(),
        Some(r"C:\repo\worktree")
    );
    assert_eq!(
        workspace_group_create_cwd(
            &snapshot.windows[0].tab_manager,
            None,
            &[second_id],
            &HashSet::from([second_id]),
        )
        .as_deref(),
        Some("C:/first")
    );
    assert_eq!(
        workspace_group_create_cwd(
            &snapshot.windows[0].tab_manager,
            Some("   ".to_string()),
            &[second_id],
            &HashSet::new(),
        ),
        default_workspace_directory()
    );
}

#[test]
fn workspace_group_icon_normalization_uses_the_frozen_host_catalog() {
    assert_eq!(
        normalized_workspace_group_icon_symbol(Some("  folder.fill  ")).as_deref(),
        Some("folder.fill")
    );
    assert_eq!(
        normalized_workspace_group_icon_symbol(Some("server.rack")).as_deref(),
        Some("server.rack")
    );
    assert_eq!(
        normalized_workspace_group_icon_symbol(Some("not.an.sf.symbol")),
        None
    );
    assert_eq!(normalized_workspace_group_icon_symbol(Some("  ")), None);
    assert_eq!(normalized_workspace_group_icon_symbol(None), None);
}

#[test]
fn workspace_group_move_index_matches_canonical_number_coercion() {
    for (value, expected) in [
        (json!(1.9), Some(1)),
        (json!(-2.9), Some(-2)),
        (json!(true), Some(1)),
        (json!(false), Some(0)),
        (json!("42"), Some(42)),
        (json!(" 42 "), None),
        (json!(1e30), Some(i64::MAX)),
        (json!(-1e30), Some(i64::MIN)),
    ] {
        let params = serde_json::Map::from_iter([("to_index".to_string(), value)]);
        assert_eq!(workspace_group_move_index_param(&params), expected);
    }
}

#[test]
fn workspace_group_parameter_errors_use_foundation_style_descriptions() {
    assert_eq!(workspace_group_parameter_description(&json!("raw")), "raw");
    assert_eq!(
        workspace_group_parameter_description(&json!([1, "two", true])),
        r#"[1, "two", 1]"#
    );
    assert_eq!(
        workspace_group_parameter_description(&json!({"name": "build"})),
        r#"["name": "build"]"#
    );
}

#[test]
fn workspace_select_focus_intent_targets_owning_window_only() {
    let mut snapshot = test_snapshot();
    let mut background = snapshot.windows[0].clone();
    background.window_id = Some("window-b".to_string());
    snapshot.windows.push(background);

    assert_eq!(
        workspace_select_focus_selector(&snapshot, 1),
        Some("window-b")
    );
    assert_eq!(workspace_select_focus_selector(&snapshot, 9), None);
}

#[test]
fn workspace_events_observe_background_window_lifecycle_changes() {
    let previous = test_snapshot();
    let mut current = previous.clone();
    let mut background = current.windows[0].clone();
    background.window_id = Some("window-b".to_string());
    current.windows.push(background.clone());
    let mut previous_with_background = previous;
    previous_with_background.windows.push(background);
    current.windows[1].tab_manager.workspaces[0].custom_title = Some("Renamed".to_string());

    assert_ne!(
        session_event_summaries(&previous_with_background),
        session_event_summaries(&current),
        "background changes must reach lifecycle event derivation"
    );
}

#[test]
fn string_map_param_accepts_string_environment_aliases() {
    let params = serde_json::Map::from_iter([(
        "env".to_string(),
        json!({
            "CMUX_FORK": "1",
            "EMPTY": "   ",
            "NUMBER": 7,
            "  TRIMMED_KEY  ": " value ",
        }),
    )]);
    let map = string_map_param(&params, &["environment", "env"]).expect("env map");
    assert_eq!(map.get("CMUX_FORK").map(String::as_str), Some("1"));
    assert_eq!(map.get("TRIMMED_KEY").map(String::as_str), Some("value"));
    assert!(!map.contains_key("EMPTY"));
    assert!(!map.contains_key("NUMBER"));
}

#[test]
fn workspace_create_cwd_preserves_raw_fallback_but_trims_working_directory() {
    let inherited = Some("C:/inherited");
    assert_eq!(
        workspace_create_cwd_param(
            &serde_json::Map::from_iter([("cwd".to_string(), json!("  C:/raw  "))]),
            inherited,
        )
        .unwrap(),
        Some("  C:/raw  ".to_string())
    );
    assert_eq!(
        workspace_create_cwd_param(
            &serde_json::Map::from_iter([
                ("working_directory".to_string(), json!("  C:/trimmed  ")),
                ("cwd".to_string(), json!({"invalid": true})),
            ]),
            inherited,
        )
        .unwrap(),
        Some("C:/trimmed".to_string())
    );
    assert!(workspace_create_cwd_param(
        &serde_json::Map::from_iter([("cwd".to_string(), json!(42))]),
        inherited,
    )
    .is_err());
}

#[test]
fn workspace_create_environment_sanitizers_preserve_values_and_differ() {
    let params = serde_json::Map::from_iter([
        (
            "initial_env".to_string(),
            json!({"  KEEP  ": "  value  ", "EMPTY": "", " ": "drop", "NUMBER": 7}),
        ),
        (
            "workspace_env".to_string(),
            json!({
                "  KEEP  ": "  value  ",
                "EMPTY": "",
                "BAD=KEY": "drop",
                "NUL\u{0000}KEY": "drop",
                "NUL_VALUE": "bad\u{0000}value"
            }),
        ),
    ]);

    assert_eq!(
        workspace_create_initial_env(&params),
        BTreeMap::from([
            ("EMPTY".to_string(), "".to_string()),
            ("KEEP".to_string(), "  value  ".to_string()),
        ])
    );
    assert_eq!(
        workspace_create_workspace_env(&params),
        BTreeMap::from([("KEEP".to_string(), "  value  ".to_string())])
    );
}

#[test]
fn raw_string_param_preserves_empty_strings_for_clearing_metadata() {
    let params = serde_json::Map::from_iter([("title".to_string(), json!(""))]);
    assert_eq!(raw_string_param(&params, &["title"]).as_deref(), Some(""));
    assert_eq!(string_param(&params, &["title"]), None);
}

#[test]
fn surface_id_from_params_accepts_ref_id_or_focused_default_not_index() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".to_string()),
            panel_ids: vec!["surface-1".to_string(), "surface-2".to_string()],
            selected_panel_id: Some("surface-2".to_string()),
            surface_kind: None,
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: None,
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));
    workspace.focused_panel_id = Some("surface-2".to_string());

    assert_eq!(
        surface_id_from_params_or_focused(
            &snapshot,
            &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
        ),
        None
    );
    assert_eq!(
        surface_id_from_params_or_focused(
            &snapshot,
            &serde_json::Map::from_iter([("surface_ref".to_string(), json!("surface:2"),)])
        ),
        Some("surface-2".to_string())
    );
    assert_eq!(
        surface_id_from_params_or_focused(
            &snapshot,
            &serde_json::Map::from_iter([("panel_id".to_string(), json!("surface-1"),)])
        ),
        Some("surface-1".to_string())
    );
    assert_eq!(
        surface_id_from_params_or_focused(&snapshot, &serde_json::Map::new()),
        Some("surface-2".to_string())
    );
}

#[test]
fn surface_id_from_params_uses_ambient_workspace_scope_for_default_surface() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("workspace-2".to_string());
    second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-2".to_string()),
            panel_ids: vec!["surface-2".to_string(), "surface-3".to_string()],
            selected_panel_id: Some("surface-3".to_string()),
            surface_kind: None,
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: None,
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));
    second.focused_panel_id = Some("surface-3".to_string());
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

    assert_eq!(
        surface_id_from_params_or_focused(&snapshot, &serde_json::Map::new()),
        Some("surface-1".to_string())
    );
    assert_eq!(
        surface_id_from_params_or_focused(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"),)])
        ),
        Some("surface-3".to_string())
    );
    assert_eq!(
        surface_id_from_params_or_focused(
            &snapshot,
            &serde_json::Map::from_iter([
                ("workspace_id".to_string(), json!("workspace-2")),
                ("surface_ref".to_string(), json!("surface:1")),
            ])
        ),
        Some("surface-2".to_string())
    );
}

#[test]
fn surface_ref_and_terminal_type_helpers_use_scoped_workspace() {
    let mut snapshot = test_snapshot();
    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
        SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".to_string()),
            panel_ids: vec!["surface-1".to_string()],
            selected_panel_id: Some("surface-1".to_string()),
            surface_kind: Some("browser".to_string()),
            markdown_file_path: None,
            file_path: None,
            diff_viewer_token: None,
            diff_viewer_request_path: None,
            browser_url: Some("https://example.com".to_string()),
            browser_proxy_url: None,
            browser_back_history: None,
            browser_forward_history: None,
            browser_omnibar_visible: None,
            browser_focus_mode_active: None,
            browser_developer_tools_visible: None,
            browser_developer_tools_panel: None,
            browser_page_zoom: None,
        },
    ));

    assert_eq!(
        surface_ref_for_panel(&snapshot, 0, "surface-1").as_deref(),
        Some("surface:1")
    );
    assert!(!surface_is_terminal(&snapshot, 0, "surface-1"));

    let workspace = snapshot.windows[0]
        .tab_manager
        .workspaces
        .first_mut()
        .unwrap();
    if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() {
        pane.surface_kind = None;
    }
    assert!(surface_is_terminal(&snapshot, 0, "surface-1"));
}

#[test]
fn terminal_key_sequence_maps_common_terminal_keys() {
    assert_eq!(terminal_key_sequence("enter"), Some("\r"));
    assert_eq!(terminal_key_sequence("ctrl+c"), Some("\x03"));
    assert_eq!(terminal_key_sequence("escape"), Some("\x1b"));
    assert_eq!(terminal_key_sequence("page-down"), Some("\x1b[6~"));
    assert_eq!(terminal_key_sequence("definitely-not-a-key"), None);
}

#[test]
fn split_orientation_from_params_accepts_cmux_aliases() {
    assert_eq!(
        split_orientation_from_params(&serde_json::Map::new()),
        Some(SessionSplitOrientation::Horizontal)
    );
    assert_eq!(
        split_orientation_from_params(&serde_json::Map::from_iter([(
            "orientation".to_string(),
            json!("vertical"),
        )])),
        Some(SessionSplitOrientation::Vertical)
    );
    assert_eq!(
        split_orientation_from_params(&serde_json::Map::from_iter([(
            "direction".to_string(),
            json!("right"),
        )])),
        Some(SessionSplitOrientation::Horizontal)
    );
    assert_eq!(
        split_orientation_from_params(&serde_json::Map::from_iter([(
            "direction".to_string(),
            json!("u"),
        )])),
        Some(SessionSplitOrientation::Vertical)
    );
    assert_eq!(
        split_orientation_from_params(&serde_json::Map::from_iter([(
            "orientation".to_string(),
            json!("diagonal"),
        )])),
        None
    );
}

#[test]
fn surface_kind_from_params_normalizes_terminal_and_known_surfaces() {
    assert_eq!(
        surface_kind_from_params(&serde_json::Map::from_iter([(
            "type".to_string(),
            json!("terminal"),
        )])),
        None
    );
    assert_eq!(
        surface_kind_from_params(&serde_json::Map::from_iter([(
            "kind".to_string(),
            json!("Browser"),
        )])),
        Some("browser".to_string())
    );
    assert_eq!(
        surface_kind_from_params(&serde_json::Map::from_iter([(
            "type".to_string(),
            json!("not-a-surface"),
        )])),
        Some("invalid".to_string())
    );
    assert_eq!(
        surface_kind_from_params(&serde_json::Map::new()),
        Some("invalid".to_string())
    );
}

#[test]
fn f64_param_accepts_numbers_and_numeric_strings() {
    assert_eq!(
        f64_param(
            &serde_json::Map::from_iter([("zoom".to_string(), json!(1.25),)]),
            &["zoom"]
        ),
        Some(1.25)
    );
    assert_eq!(
        f64_param(
            &serde_json::Map::from_iter([("scale".to_string(), json!("1.5"),)]),
            &["zoom", "scale"]
        ),
        Some(1.5)
    );
    assert_eq!(
        f64_param(
            &serde_json::Map::from_iter([("zoom".to_string(), json!("nope"),)]),
            &["zoom"]
        ),
        None
    );
}

#[test]
fn bool_param_accepts_booleans_and_common_strings() {
    assert_eq!(
        bool_param(
            &serde_json::Map::from_iter([("pinned".to_string(), json!(true),)]),
            &["pinned"]
        ),
        Some(true)
    );
    assert_eq!(
        bool_param(
            &serde_json::Map::from_iter([("unread".to_string(), json!("off"),)]),
            &["unread"]
        ),
        Some(false)
    );
    assert_eq!(
        bool_param(
            &serde_json::Map::from_iter([("pinned".to_string(), json!("maybe"),)]),
            &["pinned"]
        ),
        None
    );
}

#[test]
fn remote_tmux_creation_uses_observed_window_arrival_and_typed_rollback() {
    let pane = RemoteTmuxTarget::for_create("split-window").unwrap();
    assert_eq!(pane.rollback_operation(), "kill-pane");
    assert!(pane.permits_immediate_arrival("runtime-pane-add"));

    let window = RemoteTmuxTarget::for_create("new-window").unwrap();
    assert_eq!(window.rollback_operation(), "kill-window");
    assert!(!window.permits_immediate_arrival("runtime-window-add"));
    assert!(!window.permits_immediate_arrival("runtime-pane-add"));
    assert!(
        immediate_remote_arrival(window, "runtime-window-add", "window", "workspace", "@12")
            .is_none()
    );
    assert!(
        immediate_remote_arrival(pane, "runtime-pane-add", "window", "workspace", "%34").is_some()
    );

    let observed_window = StagedRemoteCreation {
        target: window,
        token: "@12".into(),
        window_id: "window".into(),
        workspace_id: "workspace".into(),
        target_pane_id: Some("pane-existing".into()),
        source_surface_id: Some("surface-source".into()),
        source_pane_id: Some("pane-existing".into()),
        split_orientation: None,
        focus: false,
        observation: Some(RemoteTmuxObservation {
            window_token: "@12".into(),
            pane_token: "%34".into(),
        }),
        pane_observation: None,
        arrival: None,
    };
    let arrival = observed_remote_window_arrival(&observed_window, "%34")
        .expect("observed window pane should reconcile");
    assert_eq!(arrival.window_id, "window");
    assert_eq!(arrival.workspace_id, "workspace");
    assert_eq!(arrival.pane_id, "pane-existing");
    assert_eq!(arrival.remote_session_id, "%34");
    assert!(!arrival.creates_pane);
    assert_eq!(arrival.anchor_surface_id.as_deref(), Some("surface-source"));

    let immediate_pane = StagedRemoteCreation {
        target: pane,
        token: "%34".into(),
        window_id: "window".into(),
        workspace_id: "workspace".into(),
        target_pane_id: None,
        source_surface_id: None,
        source_pane_id: None,
        split_orientation: None,
        focus: false,
        observation: None,
        pane_observation: Some("%34".into()),
        arrival: immediate_remote_arrival(pane, "runtime-pane-add", "window", "workspace", "%34"),
    };
    assert!(observed_remote_window_arrival(&immediate_pane, "%34").is_none());
}

#[test]
fn production_remote_new_window_builder_is_exact_for_focus_and_background() {
    let focused = RemoteTmuxCreateSpec {
        operation: "new-window",
        focus: true,
        source_target: Some("@7"),
        working_directory: Some("/srv/repo with spaces"),
    };
    assert_eq!(
        remote_tmux_create_argv(&focused).unwrap(),
        ["tmux new-window -a -t '@7' -c '/srv/repo with spaces' -P -F '#{window_id}\t#{pane_id}'"]
    );

    let background = RemoteTmuxCreateSpec {
        focus: false,
        ..focused
    };
    assert_eq!(
        remote_tmux_create_argv(&background).unwrap(),
        ["tmux new-window -d -a -t '@7' -c '/srv/repo with spaces' -P -F '#{window_id}\t#{pane_id}'"]
    );

    let fallback = RemoteTmuxCreateSpec {
        operation: "new-window",
        focus: false,
        source_target: None,
        working_directory: Some("/must/not/inherit"),
    };
    assert_eq!(
        remote_tmux_create_argv(&fallback).unwrap(),
        ["tmux new-window -d -a -t '{end}' -P -F '#{window_id}\t#{pane_id}'"]
    );

    let split = RemoteTmuxCreateSpec {
        operation: "split-window",
        focus: true,
        source_target: Some("@ignored"),
        working_directory: Some("/ignored"),
    };
    assert_eq!(
        remote_tmux_create_argv(&split).unwrap(),
        ["tmux split-window -P -F '#{pane_id}'"]
    );
    assert_eq!(
        remote_tmux_source_window_command("%7").unwrap(),
        ["tmux display-message -p -t '%7' '#{window_id}'"]
    );
    for invalid in ["@", "@x", "@7;echo", "@７"] {
        assert!(remote_tmux_create_argv(&RemoteTmuxCreateSpec {
            operation: "new-window",
            focus: false,
            source_target: Some(invalid),
            working_directory: None,
        })
        .is_err());
    }
}

#[test]
fn production_remote_observation_is_authoritative_retried_and_compensated() {
    let observation = parse_remote_tmux_observation("@12\t%34\n").unwrap();
    assert_eq!(observation.window_token, "@12");
    assert_eq!(observation.pane_token, "%34");
    assert_eq!(
        parse_remote_tmux_observation("@12\t%34\r\n").unwrap(),
        observation
    );
    for invalid in [
        "@12\t%34\textra\n",
        "@12x\t%34\n",
        "@12\t%34\nextra\n",
        "@12\t%x\n",
    ] {
        assert!(parse_remote_tmux_observation(invalid).is_err());
    }
    assert_eq!(
        remote_observation_action(true, None, 0),
        RemoteObservationAction::Reconcile
    );
    assert_eq!(
        remote_observation_action(true, Some("transient persistence failure"), 0),
        RemoteObservationAction::RetainAndRetry
    );
    assert_eq!(
        remote_observation_action(false, None, 0),
        RemoteObservationAction::CompensateKillWindow
    );
    assert_eq!(
        remote_observation_action(true, Some("still failing"), REMOTE_OBSERVATION_MAX_RETRIES),
        RemoteObservationAction::CompensateKillWindow
    );
    assert!(should_focus_window_after_remote_arrival(true, true));
    assert!(!should_focus_window_after_remote_arrival(true, false));
    assert!(!should_focus_window_after_remote_arrival(false, true));
}

#[test]
fn unchanged_effect_only_transition_skips_snapshot_commit_and_publication() {
    let previous = test_snapshot();
    assert!(!lifecycle_snapshot_changed(&previous, &previous));
    let mut candidate = previous.clone();
    candidate.created_at += 1;
    assert!(lifecycle_snapshot_changed(&candidate, &previous));
}

#[test]
fn lifecycle_commit_failure_keeps_model_unpublished_and_compensates_resources() {
    #[derive(Default)]
    struct CommitFailure {
        prepared: Option<AppSessionSnapshot>,
        staged: usize,
        compensated: usize,
    }
    impl pane_surface_lifecycle::LifecycleEffectExecutor for CommitFailure {
        type Error = String;

        fn prepare_transition(
            &mut self,
            candidate: &AppSessionSnapshot,
        ) -> Result<(), Self::Error> {
            self.prepared = Some(candidate.clone());
            Ok(())
        }

        fn stage(
            &mut self,
            _effect: &pane_surface_lifecycle::LifecycleEffect,
        ) -> Result<(), Self::Error> {
            self.staged += 1;
            Ok(())
        }

        fn commit_staged(&mut self) -> Result<(), Self::Error> {
            Err("injected post-stage commit failure".into())
        }

        fn rollback_staged(&mut self) -> Result<(), Self::Error> {
            self.staged = 0;
            Ok(())
        }

        fn rollback_committed(&mut self) -> Result<(), Self::Error> {
            self.compensated += self.staged;
            self.staged = 0;
            Ok(())
        }
    }

    let before = test_snapshot();
    let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
        &before,
        "pane.create",
        json!({"direction":"right","type":"terminal"})
            .as_object()
            .unwrap(),
        &pane_surface_lifecycle::LifecycleDispatchContext {
            viewport_size: Some((1_000.0, 800.0)),
            browser_enabled: true,
            dock_available: false,
            active_window_id: None,
        },
    );
    let candidate = transition.snapshot.clone();
    let effect_count = transition.effects.len();
    let mut published = before.clone();
    let mut executor = CommitFailure::default();
    assert!(pane_surface_lifecycle::commit_lifecycle_transition(
        &mut published,
        transition,
        &mut executor
    )
    .is_err());
    assert_eq!(published, before);
    assert_eq!(executor.prepared, Some(candidate));
    assert_eq!(executor.compensated, effect_count);
}

#[test]
fn lifecycle_routing_uses_active_window_and_group_manager_selected_workspace() {
    let mut snapshot = test_snapshot();
    let mut second = snapshot.windows[0].clone();
    second.window_id = Some("window-2".into());
    let mut selected = second.tab_manager.workspaces[0].clone();
    selected.workspace_id = Some("workspace-selected".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = selected.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.pane_id = Some("pane-selected".into());
    pane.panel_ids = vec!["surface-selected".into()];
    pane.selected_panel_id = Some("surface-selected".into());
    selected.focused_panel_id = Some("surface-selected".into());
    let mut anchor = selected.clone();
    anchor.workspace_id = Some("workspace-anchor".into());
    anchor.focused_panel_id = Some("surface-anchor".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = anchor.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.pane_id = Some("pane-anchor".into());
    pane.panel_ids = vec!["surface-anchor".into()];
    pane.selected_panel_id = Some("surface-anchor".into());
    second.tab_manager.workspaces = vec![selected, anchor];
    second.tab_manager.selected_workspace_index = Some(0);
    second.tab_manager.workspace_groups = Some(vec![SessionWorkspaceGroupSnapshot {
        id: "group-2".into(),
        name: "Group".into(),
        anchor_workspace_id: Some("workspace-anchor".into()),
        ..Default::default()
    }]);
    snapshot.windows.push(second);
    let context = pane_surface_lifecycle::LifecycleDispatchContext {
        viewport_size: None,
        browser_enabled: true,
        dock_available: false,
        active_window_id: Some("window-2".into()),
    };

    for params in [
        json!({}),
        json!({"window_id":null}),
        json!({"group_id":"group-2"}),
    ] {
        let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
            &snapshot,
            "surface.current",
            params.as_object().unwrap(),
            &context,
        );
        let ControlCallResult::Ok(value) = transition.result else {
            panic!("route failed")
        };
        let value = Value::from(value);
        assert_eq!(value["window_id"], "window-2");
        assert_eq!(value["workspace_id"], "workspace-selected");
    }
}

#[test]
fn lifecycle_surface_create_preserves_all_heterogeneous_kinds() {
    let context = pane_surface_lifecycle::LifecycleDispatchContext {
        viewport_size: None,
        browser_enabled: true,
        dock_available: false,
        active_window_id: None,
    };
    // Canonical surfacePanelType (ControlSurfaceContext2.swift:517-528):
    // only the recognized tokens map to non-terminal kinds; unknown tokens
    // such as projectSidebar/diff fall back to terminal (the previous
    // assertions pinned noncanonical distinct kinds for those tokens).
    for (token, expected) in [
        ("markdown", "markdown"),
        ("filePreview", "filePreview"),
        ("rightSidebarTool", "rightSidebarTool"),
    ] {
        let snapshot = test_snapshot();
        let params = json!({"pane_id":"pane-1","type":token});
        let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
            &snapshot,
            "surface.create",
            params.as_object().unwrap(),
            &context,
        );
        let ControlCallResult::Ok(value) = &transition.result else {
            panic!("{token} failed")
        };
        assert_eq!(Value::from(value.clone())["type"], expected);
        assert!(transition.effects.iter().any(|effect| matches!(
            effect,
            pane_surface_lifecycle::LifecycleEffect::UiSurfaceAttach { kind, .. }
                if kind == expected
        )));
    }
    for token in ["projectSidebar", "diff"] {
        let snapshot = test_snapshot();
        let params = json!({"pane_id":"pane-1","type":token});
        let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
            &snapshot,
            "surface.create",
            params.as_object().unwrap(),
            &context,
        );
        let ControlCallResult::Ok(value) = &transition.result else {
            panic!("{token} failed")
        };
        assert_eq!(Value::from(value.clone())["type"], "terminal");
        assert!(transition.effects.iter().any(|effect| matches!(
            effect,
            pane_surface_lifecycle::LifecycleEffect::TerminalCreate { .. }
        )));
    }

    let invalid = pane_surface_lifecycle::dispatch_lifecycle_request(
        &test_snapshot(),
        "surface.create",
        json!({"pane_id":"pane-1","type":"agentSession","renderer_kind":"canvas"})
            .as_object()
            .unwrap(),
        &context,
    );
    assert!(
        matches!(invalid.result, ControlCallResult::Err { code, .. } if code == "invalid_params")
    );
}

#[test]
fn lifecycle_tab_refs_share_the_surface_handle_number() {
    let mut registry = ControlHandleRegistry::default();
    let surface_ref = registry.mint("surface", "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(surface_ref, "surface:1");
    let tab_ref = tab_ref_from_surface_ref(&surface_ref);
    assert_eq!(tab_ref, "tab:1");
    let normalized = surface_ref_from_tab_ref(&tab_ref).unwrap();
    assert_eq!(normalized, "surface:1");
    assert_eq!(
        registry.resolve("surface", &normalized).as_deref(),
        Some("550e8400-e29b-41d4-a716-446655440000")
    );
    assert_eq!(
        registry.resolve("surface", "550e8400-e29b-41d4-a716-446655440000"),
        None,
        "UUIDs bypass the ref registry and remain unchanged"
    );
}

#[test]
fn shell_execute_codes_only_succeed_above_documented_error_range() {
    for code in [isize::MIN, 0, 2, 31, 32] {
        assert!(!shell_execute_succeeded(code), "code {code}");
    }
    for code in [33, 42, isize::MAX] {
        assert!(shell_execute_succeeded(code), "code {code}");
    }
}

#[test]
fn lifecycle_result_decoration_covers_source_created_and_tab_id_families() {
    let mut value = json!({
        "window_id": "window-current",
        "source_window_id": "window-source",
        "workspace_id": "workspace-current",
        "source_workspace_id": "workspace-source",
        "created_workspace_id": "workspace-created",
        "pane_id": "pane-current",
        "surface_id": "surface-current",
        "created_surface_id": "surface-created",
        "tab_id": "surface-current",
        "created_tab_id": "surface-created",
        "nullable": { "created_surface_id": null },
        "rows": [{ "id": "surface-row" }]
    });
    let mut registry = ControlHandleRegistry::default();
    decorate_lifecycle_value_refs(&mut value, &mut |kind, id| registry.mint(kind, id));

    assert_eq!(value["window_ref"], "window:1");
    assert_eq!(value["source_window_ref"], "window:2");
    assert_eq!(value["workspace_ref"], "workspace:1");
    assert_eq!(value["source_workspace_ref"], "workspace:2");
    assert_eq!(value["created_workspace_ref"], "workspace:3");
    assert_eq!(value["pane_ref"], "pane:1");
    assert_eq!(value["surface_ref"], "surface:1");
    assert_eq!(value["created_surface_ref"], "surface:2");
    assert_eq!(value["tab_ref"], "tab:1");
    assert_eq!(value["created_tab_ref"], "tab:2");
    assert_eq!(value["nullable"]["created_surface_ref"], Value::Null);
    assert_eq!(value["rows"][0]["ref"], "surface:3");
}

#[test]
fn lifecycle_error_ref_decoration_is_restricted_to_canonical_cases() {
    // Canonical: only the tab.action/surface.action Tab-not-found error
    // data carries refs (ControlCommandCoordinator+SystemTabAction.swift:39-48)
    // and surface.report_pwd's not_found errors carry the ref-bearing
    // requested-identity block
    // (ControlCommandCoordinator+Surface3.swift:240-251,365-375); every
    // other lifecycle error keeps plain ids in its data (e.g.
    // surface.respawn, ControlCommandCoordinator+Surface.swift:449-473).
    // Success payloads stay decorated.
    let mut registry = ControlHandleRegistry::default();
    let mut mint = |kind: &'static str, id: &str| registry.mint(kind, id);
    let error = |code: &str, message: &str, data: Value| ControlCallResult::Err {
        code: code.into(),
        message: message.into(),
        data: JsonValue::try_from(data).ok(),
    };
    let data_of = |result: &ControlCallResult| -> Value {
        let ControlCallResult::Err {
            data: Some(data), ..
        } = result
        else {
            panic!("expected error data");
        };
        Value::from(data.clone())
    };

    let mut respawn_error = error(
        "not_found",
        "Surface not found for the given surface_id",
        json!({"surface_id":"550e8400-e29b-41d4-a716-446655440000"}),
    );
    assert!(
        decorate_lifecycle_result_refs_with("surface.respawn", &mut respawn_error, &mut mint)
            .is_none()
    );
    assert!(
        data_of(&respawn_error).get("surface_ref").is_none(),
        "non-canonical error decoration for surface.respawn"
    );

    let mut close_error = error(
        "not_found",
        "Surface not found",
        json!({"surface_id":"550e8400-e29b-41d4-a716-446655440000"}),
    );
    decorate_lifecycle_result_refs_with("surface.close", &mut close_error, &mut mint);
    assert!(data_of(&close_error).get("surface_ref").is_none());

    for method in ["tab.action", "surface.action"] {
        let mut tab_not_found = error(
            "not_found",
            "Tab not found",
            json!({
                "surface_id":"550e8400-e29b-41d4-a716-446655440000",
                "tab_id":"550e8400-e29b-41d4-a716-446655440000"
            }),
        );
        decorate_lifecycle_result_refs_with(method, &mut tab_not_found, &mut mint);
        let data = data_of(&tab_not_found);
        assert!(data["surface_ref"].is_string(), "{method}");
        assert!(data["tab_ref"].is_string(), "{method}");

        let mut unknown_action = error(
            "invalid_params",
            "Unknown tab action",
            json!({"action":"bogus","supported_actions":[]}),
        );
        decorate_lifecycle_result_refs_with(method, &mut unknown_action, &mut mint);
        assert_eq!(
            data_of(&unknown_action),
            json!({"action":"bogus","supported_actions":[]}),
            "{method}"
        );
    }

    let mut report_error = error(
        "not_found",
        "Workspace not found",
        json!({"workspace_id":"650e8400-e29b-41d4-a716-446655440000","surface_id":null}),
    );
    decorate_lifecycle_result_refs_with("surface.report_pwd", &mut report_error, &mut mint);
    let data = data_of(&report_error);
    assert!(data["workspace_ref"].is_string());
    assert_eq!(data["surface_ref"], Value::Null);

    let mut success = ControlCallResult::Ok(
        JsonValue::try_from(json!({"surface_id":"550e8400-e29b-41d4-a716-446655440000"})).unwrap(),
    );
    let decorated = decorate_lifecycle_result_refs_with("surface.respawn", &mut success, &mut mint)
        .expect("success decoration returns the decorated payload");
    assert!(decorated["surface_ref"].is_string());
}

#[test]
fn control_pipe_name_override_is_validated_with_default_fallback() {
    let default_path = cmux_ipc::control_pipe_path(CONTROL_PIPE_BASE_NAME).unwrap();
    // Default unchanged when no override is present.
    assert_eq!(control_pipe_path_for_base(None), default_path);
    // A valid override is honored.
    assert_eq!(
        control_pipe_path_for_base(Some("cmux-test-fixture-7")),
        cmux_ipc::control_pipe_path("cmux-test-fixture-7").unwrap()
    );
    // Invalid overrides (same rules as the pipe-path builder: empty,
    // backslash, over-long) fall back to the default — never panic.
    let long = "x".repeat(300);
    for invalid in ["", "bad\\name", long.as_str()] {
        assert_eq!(
            control_pipe_path_for_base(Some(invalid)),
            default_path,
            "{invalid:?}"
        );
    }
}

#[path = "tests/pane_surface_lifecycle_red.rs"]
mod pane_surface_lifecycle_red;

#[path = "tests/window_lifecycle_red.rs"]
mod window_lifecycle_red;

#[path = "tests/differential_remediation_red.rs"]
mod differential_remediation_red;

#[path = "tests/surface_action_exhaustive_red.rs"]
mod surface_action_exhaustive_red;

#[path = "tests/workspace_action_red.rs"]
mod workspace_action_red;

#[path = "tests/workspace_group_red.rs"]
mod workspace_group_red;

#[path = "tests/dock_api_adversarial_red.rs"]
mod dock_api_adversarial_red;

#[path = "tests/dock_production_rollback_red.rs"]
mod dock_production_rollback_red;

#[path = "tests/surface_action_adversarial_red.rs"]
mod surface_action_adversarial_red;

#[path = "tests/remote_runtime_lifecycle_lease_red.rs"]
mod remote_runtime_lifecycle_lease_red;

#[path = "tests/manual_restore_effects_red.rs"]
mod manual_restore_effects_red;

#[path = "tests/terminal_create_input_red.rs"]
mod terminal_create_input_red;

#[path = "tests/terminal_set_font_red.rs"]
mod terminal_set_font_red;
