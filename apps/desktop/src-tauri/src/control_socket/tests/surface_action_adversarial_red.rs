//! Adversarial frozen-e1825d40 contracts not covered by the happy-path action matrix.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::*;
use cmux_core::session::{
    SessionSplitOrientation, SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot,
    SessionSurfaceSnapshot,
};
use cmux_core::surface_lifecycle::SurfaceLifecycleModel;
use serde_json::Map;

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
    source.kind = SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some("%source".into()),
        remote_context: None,
        arrival_generation: Some(1),
    };
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

fn remote_split_snapshot() -> AppSessionSnapshot {
    let mut snapshot = remote_action_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|surface| surface.surface_id == A)
        .unwrap()
        .kind = SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some("%34".into()),
        remote_context: None,
        arrival_generation: Some(1),
    };
    snapshot
}

fn remote_pane_create(snapshot: &AppSessionSnapshot, params: Value) -> LifecycleTransition {
    dispatch_with(snapshot, "pane.create", params, &context(true))
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
fn remote_new_window_local_bootstrap_source_falls_back_to_end_without_cwd() {
    let mut snapshot = remote_action_snapshot();
    let source = snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row.surface_id == A)
        .unwrap();
    source.kind = SessionSurfaceKindSnapshot::Terminal;
    let transition = dispatch(
        &snapshot,
        json!({"surface_id": A, "action": "new-terminal-right"}),
    );
    let remote = serialized_effect(&transition, "RemoteCreate");
    assert_eq!(remote["placement"], "end");
    assert_eq!(remote["source_remote_pane_id"], Value::Null);
    assert_eq!(remote["working_directory"], Value::Null);
    assert_eq!(remote["working_directory_source_surface_id"], Value::Null);
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
            "observation_phase": remote["observation_phase"],
            "pending_reconciliation": remote["pending_reconciliation"],
            "observation_failure_policy": remote["observation_failure_policy"],
            "commit_failure_policy": remote["commit_failure_policy"],
            "fabricated_surface_id": remote["fabricated_surface_id"],
            "fabricated_pane_id": remote["fabricated_pane_id"]
        }),
        json!({
            "arrival_policy": "runtime-window-add",
            "observation_source": "tmux-new-window-output",
            "observation_phase": "after-action-completion",
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

#[test]
fn production_remote_tmux_commands_cross_ssh_as_one_safe_shell_command() {
    let focused = remote_tmux_create_argv(&RemoteTmuxCreateSpec {
        operation: "new-window",
        focus: true,
        source_target: Some("@7"),
        working_directory: Some("/srv/repo with spaces/it's-here"),
    })
    .unwrap();
    assert_eq!(
        focused,
        vec![
            "tmux new-window -a -t '@7' -c '/srv/repo with spaces/it'\"'\"'s-here' \
             -P -F '#{window_id}\t#{pane_id}'"
                .replace("\n", "")
        ],
        "ssh joins multiple argv entries into an unquoted remote shell command"
    );
}

#[test]
fn production_remote_split_format_crosses_ssh_as_one_safe_shell_command() {
    let split = remote_tmux_create_argv(&RemoteTmuxCreateSpec {
        operation: "split-window",
        focus: true,
        source_target: None,
        working_directory: None,
    })
    .unwrap();
    assert_eq!(
        split,
        vec!["tmux split-window -P -F '#{pane_id}'"],
        "split output format must also survive the remote shell"
    );
}

#[test]
fn production_remote_tmux_command_omits_unusable_cwd_and_trims_safe_cwd() {
    let placement_only =
        vec!["tmux new-window -d -a -t '@7' -P -F '#{window_id}\t#{pane_id}'".to_string()];
    let unusable = [
        None,
        Some(""),
        Some("   "),
        Some("/srv/repo\rbreak"),
        Some("/srv/repo\nbreak"),
        Some("/srv/repo\0break"),
        Some("/srv/repo\u{0007}break"),
    ];
    let actual = unusable.map(|working_directory| {
        remote_tmux_create_argv(&RemoteTmuxCreateSpec {
            operation: "new-window",
            focus: false,
            source_target: Some("@7"),
            working_directory,
        })
        .unwrap_or_else(|error| vec![format!("ERROR: {error}")])
    });
    assert!(
        actual.iter().all(|command| command == &placement_only),
        "{actual:?}"
    );

    assert_eq!(
        remote_tmux_create_argv(&RemoteTmuxCreateSpec {
            operation: "new-window",
            focus: false,
            source_target: Some("@7"),
            working_directory: Some("  /srv/repo with spaces  "),
        })
        .unwrap(),
        ["tmux new-window -d -a -t '@7' -c '/srv/repo with spaces' -P -F '#{window_id}\t#{pane_id}'"]
    );
}

#[test]
fn production_remote_source_lookup_uses_the_safe_command_builder() {
    let production = include_str!("../../control_socket.rs");
    assert!(
        production.contains("remote_tmux_source_window_command("),
        "source-window lookup needs the same safe single-command builder seam"
    );
    assert!(
        !production.contains("pane_token,\n                                \"#{window_id}\""),
        "raw tmux formats must never be passed as ssh argv"
    );
}

#[test]
fn deferred_remote_arrival_is_flushed_only_after_action_completion_publication() {
    let production = include_str!("../../control_socket.rs");
    let executor_start = production
        .find("impl pane_surface_lifecycle::LifecycleEffectExecutor")
        .unwrap();
    let handler_start = production
        .find("fn handle_pane_surface_lifecycle_request")
        .unwrap();
    let executor = &production[executor_start..handler_start];
    assert!(
        !executor.contains("schedule_remote_window_reconciliation("),
        "commit_staged may enqueue identity, but must not spawn reconciliation"
    );

    let handler_end = production[handler_start..]
        .find("\nfn commit_runtime_arrival_for_control")
        .map(|offset| handler_start + offset)
        .unwrap();
    let handler = &production[handler_start..handler_end];
    let completion_publication = handler
        .rfind("for completion in completion_events")
        .expect("handler must publish action completion events");
    let deferred_flush = handler
        .find("executor.flush_deferred_remote_reconciliations(")
        .expect("handler needs an explicit deferred reconciliation queue seam");
    assert!(
        deferred_flush > completion_publication,
        "deferred surface.created must be impossible before surface.action completion"
    );
}

#[test]
fn production_arrival_commit_has_typed_noop_and_compensation_outcomes() {
    let production = include_str!("../../control_socket.rs");
    let missing = [
        "enum RuntimeArrivalCommitOutcome",
        "Committed",
        "DuplicateOrStale",
        "SourceMissing",
        "Result<RuntimeArrivalCommitOutcome, String>",
    ]
    .into_iter()
    .filter(|contract| !production.contains(contract))
    .collect::<Vec<_>>();
    assert!(missing.is_empty(), "missing typed contracts: {missing:?}");
}

#[test]
fn production_arrival_scheduler_only_compensates_source_loss_or_final_failure() {
    let production = include_str!("../../control_socket.rs");
    let scheduler_start = production
        .find("fn schedule_remote_window_reconciliation")
        .unwrap();
    let scheduler_end = production[scheduler_start..]
        .find("\nfn should_focus_window_after_remote_arrival")
        .map(|offset| scheduler_start + offset)
        .unwrap();
    let scheduler = &production[scheduler_start..scheduler_end];
    assert!(scheduler.contains("RuntimeArrivalCommitOutcome::Committed"));
    assert!(scheduler.contains("RuntimeArrivalCommitOutcome::DuplicateOrStale"));
    assert!(scheduler.contains("RuntimeArrivalCommitOutcome::SourceMissing"));
    assert!(
        scheduler.contains("DuplicateOrStale => return"),
        "duplicate/stale callbacks are successful idempotent no-ops"
    );
    assert!(
        scheduler.contains("SourceMissing") && scheduler.contains("CompensateKillWindow"),
        "only source loss or exhausted commit failure may compensate kill-window"
    );
}

#[test]
fn remote_pane_create_resolves_terminal_source_and_direction_before_routing() {
    let snapshot = remote_split_snapshot();
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (direction, tmux_flag, orientation) in
        [("right", "-h", "horizontal"), ("down", "-v", "vertical")]
    {
        let transition = remote_pane_create(
            &snapshot,
            json!({"surface_id": A, "direction": direction, "type": "terminal"}),
        );
        let remote = serialized_effect(&transition, "RemoteCreate");
        actual.push(json!({
            "source_surface_id": remote["source_surface_id"],
            "source_remote_pane_id": remote["source_remote_pane_id"],
            "split_direction": remote["split_direction"],
            "split_orientation": remote["split_orientation"],
        }));
        expected.push(json!({
            "source_surface_id": A,
            "source_remote_pane_id": "%34",
            "split_direction": tmux_flag,
            "split_orientation": orientation,
        }));
    }
    assert_eq!(actual, expected);
}

#[test]
fn production_remote_split_command_targets_the_resolved_tmux_pane() {
    assert_eq!(
        remote_tmux_create_argv(&RemoteTmuxCreateSpec {
            operation: "split-window",
            focus: false,
            source_target: Some("@7.%34"),
            working_directory: None,
        })
        .unwrap(),
        ["tmux split-window -h -t '@7.%34' -P -F '#{pane_id}'"]
    );

    let production = include_str!("../../control_socket.rs");
    assert!(production.contains("RemoteTmuxSplitDirection"));
    assert!(production.contains("split_direction: RemoteTmuxSplitDirection"));
    assert!(production.contains("valid_tmux_split_target"));
}

#[test]
fn production_remote_split_observation_is_strict_before_stage_or_rollback() {
    let parse_contract = |output: &str| {
        let line = output
            .strip_suffix("\r\n")
            .or_else(|| output.strip_suffix('\n'))
            .unwrap_or(output);
        !line.contains(['\r', '\n']) && valid_tmux_identity(line, '%')
    };
    assert!(parse_contract("%34\n"));
    assert!(parse_contract("%34\r\n"));
    for invalid in [
        "",
        "%",
        "%x",
        "%34x",
        "%34\textra",
        "%34\nextra\n",
        "%34\r",
        "%34\u{0007}",
        "@34\n",
    ] {
        assert!(!parse_contract(invalid), "{invalid:?}");
    }

    let production = include_str!("../../control_socket.rs");
    assert!(production.contains("fn parse_remote_tmux_pane_observation("));
    assert!(
        production.contains("parse_remote_tmux_pane_observation(&raw_output)"),
        "split identity must be parsed before immediate arrival is staged"
    );
}

#[test]
fn remote_split_observation_is_deferred_through_the_handler_queue() {
    let production = include_str!("../../control_socket.rs");
    let executor_start = production
        .find("impl pane_surface_lifecycle::LifecycleEffectExecutor")
        .unwrap();
    let handler_start = production
        .find("fn handle_pane_surface_lifecycle_request")
        .unwrap();
    let executor = &production[executor_start..handler_start];
    assert!(executor.contains("deferred_remote_reconciliations.extend"));
    assert!(
        !executor.contains("commit_runtime_arrival_for_control(self.app, arrival.clone())"),
        "split observations must not reconcile inside commit_staged"
    );
    let handler = &production[handler_start..];
    let completion = handler.find("for completion in completion_events").unwrap();
    let flush = handler
        .find("executor.flush_deferred_remote_reconciliations(")
        .unwrap();
    assert!(flush > completion);
}

#[test]
fn remote_pane_create_rejects_missing_explicit_source_before_effects() {
    let snapshot = remote_split_snapshot();
    let missing = remote_pane_create(
        &snapshot,
        json!({"surface_id": "40000000-0000-0000-0000-000000000099", "direction": "right"}),
    );
    assert_error(&missing, "not_found", "No source surface to split");
    assert!(missing.effects.is_empty());
}

#[test]
fn remote_pane_create_routes_only_terminal_and_preserves_browser_behavior() {
    let transition = remote_pane_create(
        &remote_split_snapshot(),
        json!({
            "surface_id": A,
            "direction": "right",
            "type": "browser",
            "url": "https://split.test"
        }),
    );
    assert!(transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::BrowserAttach { .. })));
    assert!(!transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })));
    assert_ne!(transition.snapshot, remote_split_snapshot());
}

#[test]
fn remote_pane_create_rejects_insert_first_and_unsupported_options_before_effects() {
    let snapshot = remote_split_snapshot();
    let cases = [
        json!({"surface_id": A, "direction": "left"}),
        json!({"surface_id": A, "direction": "up"}),
        json!({"surface_id": A, "direction": "right", "working_directory": "/srv/repo"}),
        json!({"surface_id": A, "direction": "right", "initial_command": "echo hi"}),
        json!({"surface_id": A, "direction": "right", "tmux_start_command": "tmux attach"}),
        json!({"surface_id": A, "direction": "right", "startup_environment": {"A":"B"}}),
        json!({"surface_id": A, "direction": "right", "initial_env": {"A":"B"}}),
        json!({"surface_id": A, "direction": "right", "initial_divider_position": 0.4}),
    ];
    let actual = cases.map(|params| {
        let transition = remote_pane_create(&snapshot, params);
        (
            matches!(transition.result, ControlCallResult::Err { ref code, .. } if code == "invalid_params"),
            transition.snapshot == snapshot,
            transition.effects.is_empty(),
        )
    });
    assert_eq!(actual, [(true, true, true); 8]);

    let left = remote_pane_create(&snapshot, json!({"surface_id": A, "direction": "left"}));
    let data = assert_error(
        &left,
        "invalid_params",
        "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): direction=left/up",
    );
    assert_eq!(data["unsupported"], json!(["direction=left/up"]));
    assert_eq!(data["routed_target"], "remote-tmux");
}

#[test]
fn remote_pane_create_reports_all_unsupported_options_in_canonical_order() {
    let snapshot = remote_split_snapshot();
    let transition = remote_pane_create(
        &snapshot,
        json!({
            "surface_id": A,
            "direction": "left",
            "working_directory": "/srv/repo",
            "initial_command": "echo hi",
            "tmux_start_command": "tmux attach",
            "startup_environment": {"A":"B"},
            "initial_divider_position": 0.4
        }),
    );
    let data = assert_error(
        &transition,
        "invalid_params",
        "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): direction=left/up, working_directory, initial_command, tmux_start_command, startup_environment, initial_divider_position",
    );
    assert_eq!(
        data["unsupported"],
        json!([
            "direction=left/up",
            "working_directory",
            "initial_command",
            "tmux_start_command",
            "startup_environment",
            "initial_divider_position"
        ])
    );

    for startup_environment in [Value::Null, json!({})] {
        let accepted = remote_pane_create(
            &snapshot,
            json!({
                "surface_id": A,
                "direction": "right",
                "startup_environment": startup_environment
            }),
        );
        assert!(matches!(accepted.result, ControlCallResult::Ok(_)));
    }
}

#[test]
fn remote_pane_create_unsupported_options_use_coordinator_parsed_values() {
    let snapshot = remote_split_snapshot();
    let omitted = [
        json!({"working_directory": null}),
        json!({"working_directory": ""}),
        json!({"working_directory": "   \r\n"}),
        json!({"initial_command": null}),
        json!({"initial_command": ""}),
        json!({"initial_command": "   "}),
        json!({"tmux_start_command": null}),
        json!({"tmux_start_command": ""}),
        json!({"command": "ignored legacy alias"}),
        json!({"initial_divider_position": null}),
        json!({"startup_environment": null}),
        json!({"startup_environment": {}}),
        json!({"startup_environment": {"   ": "ignored"}}),
        json!({"startup_environment": {"   ": "ignored"}, "initial_env": {"A": "B"}}),
    ];
    let actual = omitted.map(|extra| {
        let mut params = json!({"surface_id": A, "direction": "right"});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let transition = remote_pane_create(&snapshot, params);
        (
            matches!(transition.result, ControlCallResult::Ok(_)),
            transition.snapshot == snapshot,
            transition
                .effects
                .iter()
                .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })),
        )
    });
    assert_eq!(actual, [(true, true, true); 14]);

    for params in [
        json!({"surface_id": A, "direction": "right", "initial_env": {"  A  ": ""}}),
        json!({"surface_id": A, "direction": "right", "startup_environment": {"A": ""}}),
        json!({"surface_id": A, "direction": "right", "startup_environment": null, "initial_env": {"A": "B"}}),
    ] {
        let transition = remote_pane_create(&snapshot, params);
        let data = assert_error(
            &transition,
            "invalid_params",
            "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): startup_environment",
        );
        assert_eq!(data["unsupported"], json!(["startup_environment"]));
    }
}

#[test]
fn mirror_unsupported_options_precede_connection_and_live_source_routing() {
    let mut disconnected = remote_split_snapshot();
    let workspace = &mut disconnected.windows[0].tab_manager.workspaces[0];
    workspace.remote.as_mut().unwrap().connected = false;
    workspace.remote.as_mut().unwrap().state = "reconnecting".into();
    let transition = remote_pane_create(
        &disconnected,
        json!({"surface_id": A, "direction": "left", "working_directory": "/srv/repo"}),
    );
    let data = assert_error(
        &transition,
        "invalid_params",
        "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): direction=left/up, working_directory",
    );
    assert_eq!(
        data["unsupported"],
        json!(["direction=left/up", "working_directory"])
    );
    assert_eq!(transition.snapshot, disconnected);
    assert!(transition.effects.is_empty());

    let mut bootstrap = remote_split_snapshot();
    bootstrap.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|surface| surface.surface_id == A)
        .unwrap()
        .kind = SessionSurfaceKindSnapshot::Terminal;
    let transition = remote_pane_create(
        &bootstrap,
        json!({"surface_id": A, "direction": "right", "initial_command": "echo hi"}),
    );
    assert_error(
        &transition,
        "invalid_params",
        "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): initial_command",
    );
    assert_eq!(transition.snapshot, bootstrap);
    assert!(transition.effects.is_empty());
}

#[test]
fn disconnected_or_reconnecting_mirror_split_never_creates_a_local_orphan() {
    for state in ["disconnected", "reconnecting"] {
        let mut snapshot = remote_split_snapshot();
        let remote = snapshot.windows[0].tab_manager.workspaces[0]
            .remote
            .as_mut()
            .unwrap();
        remote.connected = false;
        remote.state = state.into();
        let transition = remote_pane_create(
            &snapshot,
            json!({"surface_id": A, "direction": "right", "type": "terminal"}),
        );
        assert_error(&transition, "internal_error", "Failed to create pane");
        assert_eq!(transition.snapshot, snapshot, "{state}");
        assert!(transition.effects.is_empty(), "{state}");
    }
}

#[test]
fn remote_split_uses_tmux_active_pane_semantics_independent_of_requested_focus() {
    let snapshot = remote_split_snapshot();
    let mut plans = Vec::new();
    for requested_focus in [false, true] {
        let transition = remote_pane_create(
            &snapshot,
            json!({"surface_id": A, "direction": "right", "focus": requested_focus}),
        );
        let remote = serialized_effect(&transition, "RemoteCreate");
        plans.push(json!({
            "focus": remote["focus"],
            "focus_mode": remote["focus_mode"],
            "activate_window": remote["activate_window"],
        }));
    }
    assert_eq!(
        plans,
        vec![
            json!({"focus": true, "focus_mode": "tmux-active", "activate_window": false}),
            json!({"focus": true, "focus_mode": "tmux-active", "activate_window": false}),
        ]
    );
}

#[test]
fn production_remote_split_command_is_never_detached_by_requested_focus() {
    for requested_focus in [false, true] {
        assert_eq!(
            remote_tmux_create_argv_with_split(
                &RemoteTmuxCreateSpec {
                    operation: "split-window",
                    focus: requested_focus,
                    source_target: Some("@7.%34"),
                    working_directory: None,
                },
                Some(RemoteTmuxSplitDirection::Horizontal),
            )
            .unwrap(),
            ["tmux split-window -h -t '@7.%34' -P -F '#{pane_id}'"]
        );
    }
}

#[test]
fn remote_split_requested_focus_never_activates_the_app_window() {
    let production = include_str!("../../control_socket.rs");
    assert!(
        production.contains("remote.target == RemoteTmuxTarget::Window")
            && production.contains("should_focus_window_after_remote_arrival"),
        "requested focus may activate an app window only for remote new-window, never split-window"
    );
}

#[test]
fn mirror_workspace_terminal_split_without_live_remote_source_never_creates_local_panel() {
    let mut snapshot = remote_split_snapshot();
    snapshot.windows[0].tab_manager.workspaces[0]
        .surfaces
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|surface| surface.surface_id == A)
        .unwrap()
        .kind = SessionSurfaceKindSnapshot::Terminal;
    let transition = remote_pane_create(
        &snapshot,
        json!({"surface_id": A, "direction": "right", "type": "terminal"}),
    );
    assert_error(&transition, "internal_error", "Failed to create pane");
    assert_eq!(transition.snapshot, snapshot);
    assert!(transition.effects.is_empty());
}

#[test]
fn remote_split_source_disappearance_is_typed_for_kill_pane_compensation() {
    let production = include_str!("../../control_socket.rs");
    assert!(production.contains("arrival.source_pane_id.as_deref()"));
    assert!(production.contains("RuntimeArrivalCommitOutcome::SourceMissing"));
    assert!(production.contains("remote_tmux_kill_command(remote.target"));
    assert!(production.contains("RemoteTmuxTarget::Pane"));
}

#[test]
fn remote_split_arrival_carries_source_orientation_into_topology_and_event() {
    let lifecycle = include_str!("../pane_surface_lifecycle.rs");
    for contract in [
        "split_orientation: Option<SessionSplitOrientation>",
        "source_pane_id: Option<String>",
        "arrival.anchor_surface_id",
        "arrival.split_orientation",
    ] {
        assert!(
            lifecycle.contains(contract),
            "missing split arrival contract: {contract}"
        );
    }
    let production = include_str!("../../control_socket.rs");
    for contract in [
        "\"source_pane_id\":arrival.source_pane_id",
        "\"orientation\":arrival.split_orientation",
    ] {
        assert!(
            production.contains(contract),
            "missing split event contract: {contract}"
        );
    }
}

#[test]
fn remote_split_reconciliation_splits_the_requested_source_pane() {
    fn pane_ids(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<String> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.pane_id.iter().cloned().collect(),
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                let mut ids = pane_ids(&split.first);
                ids.extend(pane_ids(&split.second));
                ids
            }
        }
    }
    fn has_direct_pair(
        layout: &SessionWorkspaceLayoutSnapshot,
        source_pane: &str,
        created_pane: &str,
        orientation: SessionSplitOrientation,
    ) -> bool {
        let SessionWorkspaceLayoutSnapshot::Split(split) = layout else {
            return false;
        };
        let first = pane_ids(&split.first);
        let second = pane_ids(&split.second);
        let direct = first.iter().any(|id| id == source_pane)
            && second.iter().any(|id| id == created_pane)
            && split.orientation == orientation;
        direct
            || has_direct_pair(&split.first, source_pane, created_pane, orientation.clone())
            || has_direct_pair(&split.second, source_pane, created_pane, orientation)
    }

    let first_split = dispatch_with(
        &action_snapshot(),
        "pane.create",
        json!({"surface_id": A, "direction": "right", "type": "terminal"}),
        &context(true),
    );
    let created = ok(&first_split);
    let source_surface = created["surface_id"].as_str().unwrap();
    let source_pane = created["pane_id"].as_str().unwrap();
    let remote_pane = "30000000-0000-0000-0000-000000000099";
    let remote_surface = "40000000-0000-0000-0000-000000000099";
    let mut arrival = super::pane_surface_lifecycle::RuntimeArrival::remote(
        W1,
        WS1,
        remote_pane,
        remote_surface,
        "%99",
        1,
    );
    arrival.anchor_surface_id = Some(source_surface.into());
    arrival.focused = true;
    let reconciled =
        super::pane_surface_lifecycle::reconcile_runtime_arrival(&first_split.snapshot, arrival);
    let layout = reconciled.snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap();
    let topology_correct = has_direct_pair(
        layout,
        source_pane,
        remote_pane,
        SessionSplitOrientation::Horizontal,
    );
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&reconciled.snapshot).unwrap();
    assert_eq!(
        (topology_correct, model.focused_surface(WS1)),
        (true, Some(remote_surface)),
        "supported right/down arrivals split after their requested source and honor focus"
    );
}

fn created_root_divider(transition: &LifecycleTransition) -> f64 {
    let _ = ok(transition);
    let layout = transition.snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap();
    let SessionWorkspaceLayoutSnapshot::Split(split) = layout else {
        panic!("expected created split")
    };
    split.divider_position
}

#[test]
fn v2_double_parser_accepts_bool_numbers_and_strings_but_rejects_nonfinite() {
    let parsed = [
        json!(true),
        json!(false),
        json!(1),
        json!(0.4),
        json!(" 0.3 "),
        json!("NaN"),
        json!("inf"),
        json!("-inf"),
        Value::Null,
    ]
    .map(|value| {
        let params = value
            .is_null()
            .then(Map::new)
            .unwrap_or_else(|| Map::from_iter([("initial_divider_position".into(), value)]));
        v2_double_param(&params, &["initial_divider_position"])
    });
    assert_eq!(
        parsed,
        [
            Some(1.0),
            Some(0.0),
            Some(1.0),
            Some(0.4),
            Some(0.3),
            None,
            None,
            None,
            None,
        ]
    );
}

#[test]
fn local_pane_create_applies_clamped_v2_double_divider_values() {
    let cases = [
        (json!(true), 0.9),
        (json!(false), 0.1),
        (json!(1), 0.9),
        (json!(-1), 0.1),
        (json!(0.4), 0.4),
        (json!(" 0.3 "), 0.3),
        (json!("2"), 0.9),
    ];
    let actual = cases.map(|(divider, expected)| {
        let transition = dispatch_with(
            &action_snapshot(),
            "pane.create",
            json!({
                "surface_id": A,
                "direction": "right",
                "initial_divider_position": divider,
            }),
            &context(true),
        );
        let actual = matches!(transition.result, ControlCallResult::Ok(_))
            .then(|| created_root_divider(&transition))
            .unwrap_or(f64::NAN);
        (actual, expected)
    });
    assert!(
        actual
            .iter()
            .all(|(actual, expected)| (actual - expected).abs() < f64::EPSILON),
        "{actual:?}"
    );
}

#[test]
fn pane_create_rejects_nonfinite_divider_strings_before_any_route_or_mutation() {
    for snapshot in [action_snapshot(), remote_split_snapshot()] {
        for divider in ["NaN", "inf", "-inf"] {
            let transition = remote_pane_create(
                &snapshot,
                json!({
                    "surface_id": A,
                    "direction": "right",
                    "initial_divider_position": divider,
                }),
            );
            assert_error(
                &transition,
                "invalid_params",
                "initial_divider_position must be numeric",
            );
            assert_eq!(transition.snapshot, snapshot);
            assert!(transition.effects.is_empty());
        }
    }
}

#[test]
fn enabled_remote_mirror_aggregates_boolean_divider_as_unsupported() {
    for divider in [false, true] {
        let snapshot = remote_split_snapshot();
        let transition = remote_pane_create(
            &snapshot,
            json!({
                "surface_id": A,
                "direction": "right",
                "initial_divider_position": divider,
            }),
        );
        let data = assert_error(
            &transition,
            "invalid_params",
            "Not supported when targeting a remote tmux mirror workspace (the request is routed to tmux and these options cannot be applied): initial_divider_position",
        );
        assert_eq!(data["unsupported"], json!(["initial_divider_position"]));
        assert_eq!(transition.snapshot, snapshot);
        assert!(transition.effects.is_empty());
    }
}

#[test]
fn disabled_tmux_record_uses_local_pane_create_even_if_stale_connected() {
    let mut snapshot = remote_split_snapshot();
    let remote = snapshot.windows[0].tab_manager.workspaces[0]
        .remote
        .as_mut()
        .unwrap();
    remote.enabled = false;
    remote.connected = true;
    remote.state = "connected".into();
    let transition = remote_pane_create(
        &snapshot,
        json!({"surface_id": A, "direction": "right", "type": "terminal"}),
    );
    let value = ok(&transition);
    assert!(value["pane_id"].is_string());
    assert!(value["surface_id"].is_string());
    assert_ne!(transition.snapshot, snapshot);
    assert!(transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::TerminalCreate { .. })));
    assert!(!transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })));
}

#[test]
fn disabled_tmux_record_uses_local_new_terminal_right_even_if_stale_connected() {
    let mut snapshot = remote_action_snapshot();
    let remote = snapshot.windows[0].tab_manager.workspaces[0]
        .remote
        .as_mut()
        .unwrap();
    remote.enabled = false;
    remote.connected = true;
    remote.state = "connected".into();
    let transition = dispatch(
        &snapshot,
        json!({"surface_id": A, "action": "new-terminal-right", "focus": false}),
    );
    assert!(ok(&transition)["created_surface_id"].is_string());
    assert_ne!(transition.snapshot, snapshot);
    assert!(transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::TerminalCreate { .. })));
    assert!(!transition
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })));
}
