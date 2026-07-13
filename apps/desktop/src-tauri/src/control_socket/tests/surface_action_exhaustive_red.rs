//! Frozen-e1825d40 RED oracle for `surface.action` / `tab.action`.
//!
//! The public supported-action list is deliberately smaller than the accepted
//! compatibility-alias set.  Keep both lists explicit: changing either is a
//! wire-contract change.

use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleEffect, LifecycleTransition,
};
use super::*;
use cmux_core::session::{
    SessionSurfaceKindSnapshot, SessionSurfaceMetadataSnapshot, SessionSurfaceSnapshot,
};
use cmux_core::surface_lifecycle::SurfaceLifecycleModel;
use std::collections::BTreeSet;

const WINDOW: &str = "10000000-0000-0000-0000-000000000001";
const WORKSPACE: &str = "20000000-0000-0000-0000-000000000001";
const PANE: &str = "30000000-0000-0000-0000-000000000001";
const PINNED: &str = "40000000-0000-0000-0000-000000000001";
const TERMINAL: &str = "40000000-0000-0000-0000-000000000002";
const BROWSER: &str = "40000000-0000-0000-0000-000000000003";
const TAIL: &str = "40000000-0000-0000-0000-000000000004";

const SUPPORTED_ACTIONS: [&str; 17] = [
    "rename",
    "clear_name",
    "close_left",
    "close_right",
    "close_others",
    "new_terminal_right",
    "new_browser_right",
    "reload",
    "duplicate",
    "move_to_new_workspace",
    "detach_to_workspace",
    "detach_to_new_workspace",
    "pin",
    "unpin",
    "mark_read",
    "mark_unread",
    "toggle_full_width_tab",
];

fn context(browser_enabled: bool) -> LifecycleDispatchContext {
    LifecycleDispatchContext {
        viewport_size: Some((1200.0, 800.0)),
        browser_enabled,
        dock_available: true,
        active_window_id: Some(WINDOW.into()),
    }
}

fn action_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let window = &mut snapshot.windows[0];
    window.window_id = Some(WINDOW.into());
    window.selected_workspace_id = Some(WORKSPACE.into());
    let workspace = &mut window.tab_manager.workspaces[0];
    workspace.workspace_id = Some(WORKSPACE.into());
    workspace.focused_panel_id = Some(BROWSER.into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("action fixture pane")
    else {
        unreachable!()
    };
    pane.pane_id = Some(PANE.into());
    pane.panel_ids = vec![PINNED.into(), TERMINAL.into(), BROWSER.into(), TAIL.into()];
    pane.selected_panel_id = Some(BROWSER.into());
    workspace.surfaces = Some(vec![
        surface(PINNED, SessionSurfaceKindSnapshot::Terminal, true),
        surface(TERMINAL, SessionSurfaceKindSnapshot::Terminal, false),
        surface(
            BROWSER,
            SessionSurfaceKindSnapshot::Browser {
                url: Some("https://example.test/docs".into()),
                profile: None,
                proxy_url: None,
                back_history: None,
                forward_history: None,
                omnibar_visible: None,
                focus_mode_active: None,
                developer_tools_visible: None,
                developer_tools_panel: None,
                page_zoom: None,
            },
            false,
        ),
        surface(TAIL, SessionSurfaceKindSnapshot::Terminal, false),
    ]);
    snapshot
}

fn surface(id: &str, kind: SessionSurfaceKindSnapshot, pinned: bool) -> SessionSurfaceSnapshot {
    SessionSurfaceSnapshot {
        surface_id: id.into(),
        pane_id: PANE.into(),
        generation: 1,
        kind,
        metadata: SessionSurfaceMetadataSnapshot {
            pinned,
            ..Default::default()
        },
        terminal_startup: None,
    }
}

fn transition_with_context(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: Value,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        method,
        params.as_object().expect("object params"),
        context,
    )
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    transition_with_context(snapshot, method, params, &context(true))
}

fn ok_value(transition: &LifecycleTransition) -> Value {
    let ControlCallResult::Ok(value) = &transition.result else {
        panic!("expected success, got {:?}", transition.result)
    };
    value.clone().into()
}

fn error_parts(transition: &LifecycleTransition) -> (&str, &str, Value) {
    let ControlCallResult::Err {
        code,
        message,
        data,
    } = &transition.result
    else {
        panic!("expected error, got {:?}", transition.result)
    };
    (
        code,
        message,
        data.clone().map(Value::from).unwrap_or(Value::Null),
    )
}

fn assert_error_pair(transition: &LifecycleTransition, code: &str, message: &str) {
    let (actual_code, actual_message, _) = error_parts(transition);
    assert_eq!((actual_code, actual_message), (code, message));
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("object payload")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_base_identity(value: &Value, action: &str, surface_id: &str) {
    assert_eq!(value["action"], action);
    assert_eq!(value["window_id"], WINDOW);
    assert_eq!(value["workspace_id"], WORKSPACE);
    assert_eq!(value["pane_id"], PANE);
    assert_eq!(value["surface_id"], surface_id);
    assert_eq!(value["tab_id"], surface_id);
}

#[test]
fn action_methods_have_distinct_socket_v2_entries_but_one_shared_dispatcher() {
    for method in ["surface.action", "tab.action"] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method));
        assert_eq!(
            control_request_route_for_method(method),
            ControlRequestRoute::PaneSurfaceLifecycle
        );
        let result = transition(
            &action_snapshot(),
            method,
            json!({"surface_id": BROWSER, "action": "clear-name"}),
        );
        assert_base_identity(&ok_value(&result), "clear_name", BROWSER);
    }
}

#[test]
fn unknown_action_has_exact_ordered_public_supported_actions_and_normalized_action() {
    let result = transition(
        &action_snapshot(),
        "surface.action",
        json!({"surface_id": BROWSER, "action": "  Future-Action  "}),
    );
    let (code, message, data) = error_parts(&result);
    assert_eq!(code, "invalid_params");
    assert_eq!(message, "Unknown tab action");
    assert_eq!(data["action"], "future_action");
    assert_eq!(data["supported_actions"], json!(SUPPORTED_ACTIONS));
    assert_eq!(
        data.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["action", "supported_actions"]
    );
}

#[test]
fn action_validation_and_target_resolution_follow_canonical_precedence() {
    let snapshot = action_snapshot();
    assert_error_pair(
        &transition(&snapshot, "surface.action", json!({})),
        "invalid_params",
        "Missing action",
    );

    let mut no_windows = snapshot.clone();
    no_windows.windows.clear();
    assert_error_pair(
        &transition(&no_windows, "surface.action", json!({"action": "pin"})),
        "unavailable",
        "TabManager not available",
    );

    let focused = transition(
        &snapshot,
        "surface.action",
        json!({"workspace_id": WORKSPACE, "action": "mark-read"}),
    );
    assert_base_identity(&ok_value(&focused), "mark_read", BROWSER);

    let mut no_focus = snapshot.clone();
    no_focus.windows[0].tab_manager.workspaces[0].focused_panel_id = None;
    assert_error_pair(
        &transition(
            &no_focus,
            "surface.action",
            json!({"workspace_id": WORKSPACE, "action": "pin"}),
        ),
        "not_found",
        "No focused tab",
    );

    assert_error_pair(
        &transition(
            &snapshot,
            "surface.action",
            json!({"workspace_id": "20000000-0000-0000-0000-000000000099", "action": "pin"}),
        ),
        "not_found",
        "Workspace not found",
    );

    let tab_alias = transition(
        &snapshot,
        "surface.action",
        json!({"surface_id": TERMINAL, "tab_id": BROWSER, "action": "mark-unread"}),
    );
    assert_base_identity(&ok_value(&tab_alias), "mark_unread", TERMINAL);

    let missing = "40000000-0000-0000-0000-000000000099";
    let missing_result = transition(
        &snapshot,
        "surface.action",
        json!({"surface_id": missing, "action": "pin"}),
    );
    let (code, message, data) = error_parts(&missing_result);
    assert_eq!((code, message), ("not_found", "Tab not found"));
    assert_eq!(
        data,
        json!({
            "surface_id": missing,
            "tab_id": missing
        })
    );
}

#[test]
fn metadata_actions_and_all_compatibility_aliases_return_exact_shapes() {
    let mut snapshot = action_snapshot();
    for (raw, canonical, extra) in [
        (" ReName ", "rename", Some(("title", json!("Build logs")))),
        ("clear-name", "clear_name", None),
        ("pin", "pin", Some(("pinned", json!(true)))),
        ("unpin", "unpin", Some(("pinned", json!(false)))),
        ("mark-read", "mark_read", None),
        ("mark-unread", "mark_unread", None),
        ("mark-as-unread", "mark_as_unread", None),
    ] {
        let mut params = json!({"surface_id": TERMINAL, "action": raw});
        if canonical == "rename" {
            params["title"] = json!("  Build logs \n");
        }
        let result = transition(&snapshot, "surface.action", params);
        let value = ok_value(&result);
        assert_base_identity(&value, canonical, TERMINAL);
        let mut expected: BTreeSet<&str> = [
            "action",
            "window_id",
            "workspace_id",
            "surface_id",
            "tab_id",
            "pane_id",
        ]
        .into_iter()
        .collect();
        if let Some((key, expected_value)) = extra {
            expected.insert(key);
            assert_eq!(value[key], expected_value, "{raw}");
        }
        assert_eq!(keys(&value), expected, "exact payload keys for {raw}");
        snapshot = result.snapshot;
    }

    let renamed = SurfaceLifecycleModel::from_app_session_snapshot(&snapshot).unwrap();
    assert_eq!(
        renamed.surface(TERMINAL).unwrap().metadata.custom_title,
        None
    );
    assert!(renamed.surface(TERMINAL).unwrap().metadata.unread);
}

#[test]
fn invalid_rename_title_is_exact_and_does_not_mutate_or_emit() {
    for title in [Value::Null, json!(" \n\t ")] {
        let snapshot = action_snapshot();
        let result = transition(
            &snapshot,
            "surface.action",
            json!({"surface_id": TERMINAL, "action": "rename", "title": title}),
        );
        assert_error_pair(&result, "invalid_params", "Missing or invalid title");
        assert_eq!(result.snapshot, snapshot);
        assert!(result.events.is_empty());
        assert!(result.effects.is_empty());
    }
}

#[test]
fn presentation_aliases_cover_browser_runtime_and_full_width_result() {
    for raw in ["reload", "reload-tab"] {
        let result = transition(
            &action_snapshot(),
            "surface.action",
            json!({"surface_id": BROWSER, "action": raw}),
        );
        let value = ok_value(&result);
        assert_base_identity(&value, &raw.replace('-', "_"), BROWSER);
        let effects = serde_json::to_value(&result.effects).unwrap().to_string();
        assert!(effects.contains("BrowserReload"), "{raw}: {effects}");
        assert!(effects.contains(BROWSER), "{raw}: {effects}");
    }

    for raw in [
        "toggle-full-width-tab",
        "toggle-full-width",
        "toggle-full-width-tab-mode",
    ] {
        let result = transition(
            &action_snapshot(),
            "surface.action",
            json!({"surface_id": BROWSER, "action": raw}),
        );
        assert_eq!(ok_value(&result)["full_width_tab_mode"], true, "{raw}");
    }

    for (surface, message) in [
        (TERMINAL, "Reload is only available for browser tabs"),
        (TERMINAL, "Duplicate is only available for browser tabs"),
    ] {
        let action = if message.starts_with("Reload") {
            "reload"
        } else {
            "duplicate"
        };
        assert_error_pair(
            &transition(
                &action_snapshot(),
                "surface.action",
                json!({"surface_id": surface, "action": action}),
            ),
            "invalid_state",
            message,
        );
    }
}

#[test]
fn duplicate_aliases_create_immediately_right_and_preserve_focus_by_default() {
    for raw in ["duplicate", "duplicate-tab"] {
        let snapshot = action_snapshot();
        let result = transition(
            &snapshot,
            "surface.action",
            json!({"surface_id": BROWSER, "action": raw}),
        );
        let value = ok_value(&result);
        assert!(value["created_surface_id"].is_string());
        assert_eq!(value["created_tab_id"], value["created_surface_id"]);
        let model = SurfaceLifecycleModel::from_app_session_snapshot(&result.snapshot).unwrap();
        let pane = model.pane(PANE).unwrap();
        let browser_index = pane
            .surface_ids
            .iter()
            .position(|id| id == BROWSER)
            .unwrap();
        assert_eq!(
            pane.surface_ids[browser_index + 1],
            value["created_surface_id"]
        );
        assert_eq!(model.focused_surface(WORKSPACE), Some(BROWSER));
    }
}

#[test]
fn new_surface_aliases_insert_right_but_never_before_the_pinned_prefix() {
    for (surface_id, raw, expected_effect) in [
        (PINNED, "new-terminal-right", "terminal"),
        (PINNED, "new-terminal-to-right", "terminal"),
        (PINNED, "new-terminal-tab-to-right", "terminal"),
        (PINNED, "new-browser-right", "browser"),
        (PINNED, "new-browser-to-right", "browser"),
        (PINNED, "new-browser-tab-to-right", "browser"),
    ] {
        let result = transition(
            &action_snapshot(),
            "surface.action",
            json!({"surface_id": surface_id, "action": raw, "url": "https://new.test"}),
        );
        let value = ok_value(&result);
        let created = value["created_surface_id"].as_str().unwrap();
        let model = SurfaceLifecycleModel::from_app_session_snapshot(&result.snapshot).unwrap();
        assert_eq!(model.pane(PANE).unwrap().surface_ids[1], created, "{raw}");
        assert!(result
            .effects
            .iter()
            .any(|effect| match (expected_effect, effect) {
                ("terminal", LifecycleEffect::TerminalCreate { surface_id, .. }) =>
                    surface_id == created,
                ("browser", LifecycleEffect::BrowserAttach { surface_id, .. }) =>
                    surface_id == created,
                _ => false,
            }));
    }

    let focused = transition(
        &action_snapshot(),
        "surface.action",
        json!({"surface_id": TERMINAL, "action": "new-terminal-right", "focus": true}),
    );
    let created = ok_value(&focused)["created_surface_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let model = SurfaceLifecycleModel::from_app_session_snapshot(&focused.snapshot).unwrap();
    assert_eq!(model.focused_surface(WORKSPACE), Some(created.as_str()));
    assert!(focused.effects.iter().any(|effect| matches!(
        effect,
        LifecycleEffect::ActivateWindow { window_id } if window_id == WINDOW
    )));
}

#[test]
fn browser_disabled_actions_return_the_canonical_error_or_external_open_shape() {
    let disabled = context(false);
    let no_url = transition_with_context(
        &action_snapshot(),
        "surface.action",
        json!({"surface_id": BROWSER, "action": "new-browser-right"}),
        &disabled,
    );
    assert_error_pair(&no_url, "browser_disabled", "cmux browser is disabled");

    let opened = transition_with_context(
        &action_snapshot(),
        "surface.action",
        json!({
            "surface_id": BROWSER,
            "action": "new-browser-right",
            "url": "https://external.test/path"
        }),
        &disabled,
    );
    assert_eq!(
        ok_value(&opened),
        json!({
            "window_id": WINDOW,
            "workspace_id": Value::Null,
            "pane_id": Value::Null,
            "surface_id": Value::Null,
            "created_split": false,
            "opened_externally": true,
            "browser_disabled": true,
            "placement_strategy": "external_browser_disabled",
            "url": "https://external.test/path"
        })
    );
}

#[test]
fn invalid_browser_url_precedes_browser_disabled_and_carries_raw_url() {
    let result = transition_with_context(
        &action_snapshot(),
        "surface.action",
        json!({"surface_id": BROWSER, "action": "new-browser-right", "url": "http://["}),
        &context(false),
    );
    let (code, message, data) = error_parts(&result);
    assert_eq!((code, message), ("invalid_params", "Invalid URL"));
    assert_eq!(data, json!({"url": "http://["}));
}

#[test]
fn close_aliases_return_integer_counts_skip_pins_and_preserve_one_surface() {
    for (raw, expected_closed, expected_skipped) in [
        ("close-left", 1, 1),
        ("close-to-left", 1, 1),
        ("close-right", 1, 0),
        ("close-to-right", 1, 0),
        ("close-others", 2, 1),
        ("close-other-tabs", 2, 1),
    ] {
        let result = transition(
            &action_snapshot(),
            "surface.action",
            json!({"surface_id": BROWSER, "action": raw}),
        );
        let value = ok_value(&result);
        assert_eq!(value["closed"], expected_closed);
        assert_eq!(value["skipped_pinned"], expected_skipped);
        assert!(value["closed"].is_u64());
        assert!(value["skipped_pinned"].is_u64());
        let teardowns = result
            .effects
            .iter()
            .filter_map(|effect| match effect {
                LifecycleEffect::RuntimeTeardown {
                    must_succeed,
                    phase,
                    ..
                } => Some((*must_succeed, *phase)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(teardowns.len(), expected_closed);
        assert!(teardowns
            .iter()
            .all(|(must_succeed, phase)| !must_succeed && *phase == "commit"));
        let model = SurfaceLifecycleModel::from_app_session_snapshot(&result.snapshot).unwrap();
        assert!(model.surface(PINNED).is_some());
        assert!(model.surface(BROWSER).is_some());
        assert!(!model.pane(PANE).unwrap().surface_ids.is_empty());
    }
}

#[test]
fn remote_terminal_create_is_accepted_without_fabricating_created_identity() {
    let mut snapshot = action_snapshot();
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["remote"] = json!({
        "enabled": true,
        "state": "connected",
        "connected": true,
        "transport": "tmux",
        "destination": "ssh://example.test",
        "session_id": "remote-session-1"
    });
    snapshot = serde_json::from_value(encoded).unwrap();
    let result = transition(
        &snapshot,
        "surface.action",
        json!({"surface_id": TERMINAL, "action": "new-terminal-right"}),
    );
    let value = ok_value(&result);
    assert_base_identity(&value, "new_terminal_right", TERMINAL);
    assert_eq!(value["accepted"], true);
    assert_eq!(value["routed"], "remote-tmux");
    for key in ["created_surface_id", "created_tab_id"] {
        assert!(value[key].is_null(), "{key}");
    }
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| event.name == "surface.action")
            .count(),
        1,
        "the socket action completion is independent from remote arrival"
    );
    assert!(!result
        .events
        .iter()
        .any(|event| event.name == "surface.created"));
    assert!(result
        .effects
        .iter()
        .any(|effect| matches!(effect, LifecycleEffect::RemoteCreate { .. })));
    assert_eq!(
        keys(&value),
        [
            "accepted",
            "action",
            "created_surface_id",
            "created_tab_id",
            "pane_id",
            "routed",
            "surface_id",
            "tab_id",
            "window_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn move_and_detach_aliases_preserve_surface_identity_metadata_and_shape() {
    for raw in [
        "move-to-new-workspace",
        "detach-to-workspace",
        "detach-to-new-workspace",
    ] {
        let mut snapshot = action_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0]
            .surfaces
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row.surface_id == BROWSER)
            .unwrap()
            .metadata
            .custom_title = Some("Movable docs".into());
        let result = transition(
            &snapshot,
            "surface.action",
            json!({"surface_id": BROWSER, "action": raw, "title": "Destination", "focus": false}),
        );
        let value = ok_value(&result);
        assert_eq!(value["action"], raw.replace('-', "_"));
        assert_eq!(value["source_window_id"], WINDOW);
        assert_eq!(value["source_workspace_id"], WORKSPACE);
        assert_eq!(value["surface_id"], BROWSER);
        assert_eq!(value["tab_id"], BROWSER);
        assert_eq!(value["created_workspace_id"], value["workspace_id"]);
        assert_eq!(
            keys(&value),
            [
                "action",
                "created_workspace_id",
                "pane_id",
                "source_window_id",
                "source_workspace_id",
                "surface_id",
                "tab_id",
                "window_id",
                "workspace_id",
            ]
            .into_iter()
            .collect(),
            "exact move payload for {raw}"
        );
        let model = SurfaceLifecycleModel::from_app_session_snapshot(&result.snapshot).unwrap();
        let moved = model.surface(BROWSER).unwrap();
        assert_eq!(moved.metadata.custom_title.as_deref(), Some("Movable docs"));
        assert_ne!(
            model.owner_of_surface(BROWSER).unwrap().workspace_id,
            WORKSPACE
        );
        let identity_count = result
            .snapshot
            .windows
            .iter()
            .flat_map(|window| &window.tab_manager.workspaces)
            .flat_map(|workspace| workspace.surfaces.as_deref().unwrap_or_default())
            .filter(|surface| surface.surface_id == BROWSER)
            .count();
        assert_eq!(identity_count, 1);
    }

    let mut singleton = action_snapshot();
    let workspace = &mut singleton.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.panel_ids = vec![BROWSER.into()];
    workspace
        .surfaces
        .as_mut()
        .unwrap()
        .retain(|row| row.surface_id == BROWSER);
    assert_error_pair(
        &transition(
            &singleton,
            "surface.action",
            json!({"surface_id": BROWSER, "action": "move-to-new-workspace"}),
        ),
        "invalid_state",
        "Tab cannot be moved to a new workspace because it is the only tab in its workspace",
    );
}

#[test]
fn every_success_emits_one_exact_socket_completion_and_runtime_events_are_not_faked() {
    for method in ["surface.action", "tab.action"] {
        let result = transition(
            &action_snapshot(),
            method,
            json!({"surface_id": TERMINAL, "action": "pin"}),
        );
        let completion: Vec<_> = result
            .events
            .iter()
            .filter(|event| event.name == "surface.action")
            .collect();
        assert_eq!(completion.len(), 1, "{method}");
        let event = completion[0];
        assert_eq!(event.category, "surface");
        assert_eq!(event.source, "socket.v2");
        assert_eq!(event.window_id.as_deref(), Some(WINDOW));
        assert_eq!(event.workspace_id.as_deref(), Some(WORKSPACE));
        assert_eq!(event.pane_id.as_deref(), Some(PANE));
        assert_eq!(event.surface_id.as_deref(), Some(TERMINAL));
        assert_eq!(event.payload["method"], method);
        assert_eq!(
            event.payload["params"],
            json!({"surface_id": TERMINAL, "action": "pin"})
        );
        assert_base_identity(&event.payload["result"], "pin", TERMINAL);
    }

    let reload = transition(
        &action_snapshot(),
        "surface.action",
        json!({"surface_id": BROWSER, "action": "reload"}),
    );
    assert!(!reload.events.iter().any(|event| {
        matches!(
            event.name,
            "surface.created" | "surface.closed" | "surface.moved"
        )
    }));
}
