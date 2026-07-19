//! Red suite for the window-lifecycle parity family (batch 1B).
//!
//! Contract of record: docs/parity/contracts/window_lifecycle.json (pinned
//! canonical commit e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452). Every exact
//! string below is transcribed from the contract's pinned sources, not
//! paraphrased.
//!
//! Platform adaptations pinned here (documented, not silent):
//! - Production session window ids are canonical UUIDs. Deterministic fixtures
//!   use readable labels; selector rejection is therefore pinned as shape
//!   validation (non-empty string that is not an unresolved `kind:N` ref).
//! - OS key-window state is modeled deterministically: `is_key_window` in the
//!   lifecycle event payload reflects the injected `active_window_id`, and
//!   real foregrounding/quit dialogs/redraws are pinned as effects (contract
//!   `headless_impossibility_flags`).

use super::pane_surface_lifecycle::{dispatch_lifecycle_request, LifecycleDispatchContext};
use super::window_lifecycle::{
    dispatch_window_lifecycle_request, parse_v1_window_command, resume_binding_payload,
    v1_window_reply, v1_window_request, V1WindowCommand, WindowLifecycleContext,
    WindowLifecycleEffect, WindowLifecycleTransition,
};
use super::*;
use cmux_core::session::{decode_session, encode_session};

const WINDOW_LIFECYCLE_METHODS: [&str; 7] = [
    "window.create",
    "window.close",
    "window.focus",
    "surface.refresh",
    "surface.resume.set",
    "surface.resume.get",
    "surface.resume.clear",
];

/// surface.resume.* unavailable message (Self.surfaceWindowUnavailableMessage,
/// ControlCommandCoordinator+Surface.swift:79-80) — deliberately DIFFERENT
/// from surface.refresh's "TabManager not available".
const RESUME_UNAVAILABLE: &str = "cmux window is not available. Reopen the window and try again.";

fn test_context() -> WindowLifecycleContext {
    WindowLifecycleContext {
        active_window_id: Some("window-1".to_string()),
        quit_confirmation_required: true,
        now_epoch_seconds: 1_700_000_000.5,
        new_window_id: Some("window-9".to_string()),
        new_surface_id: Some("surface-9".to_string()),
    }
}

fn dispatch(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: Value,
) -> WindowLifecycleTransition {
    dispatch_with_context(snapshot, method, params, &test_context())
}

fn dispatch_with_context(
    snapshot: &AppSessionSnapshot,
    method: &str,
    params: Value,
    context: &WindowLifecycleContext,
) -> WindowLifecycleTransition {
    let Value::Object(params) = params else {
        unreachable!("params fixture must be an object");
    };
    dispatch_window_lifecycle_request(snapshot, method, &params, context)
}

fn expect_ok(result: &ControlCallResult) -> Value {
    match result {
        ControlCallResult::Ok(payload) => Value::from(payload.clone()),
        ControlCallResult::Err { code, message, .. } => {
            panic!("expected success, got {code}: {message}")
        }
    }
}

fn expect_error(result: &ControlCallResult) -> (String, String, Option<Value>) {
    match result {
        ControlCallResult::Err {
            code,
            message,
            data,
        } => (code.clone(), message.clone(), data.clone().map(Value::from)),
        ControlCallResult::Ok(payload) => panic!("expected error, got {payload:?}"),
    }
}

/// Decorate a transition result the way the production wrapper does, using a
/// deterministic fake handle registry.
fn decorate(method: &str, result: &mut ControlCallResult) -> Option<Value> {
    decorate_lifecycle_result_refs_with(method, result, &mut |kind, id| format!("{kind}:ref:{id}"))
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .unwrap_or_else(|| panic!("expected object, got {value}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

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

fn mark_remote_workspace(snapshot: AppSessionSnapshot, window_index: usize) -> AppSessionSnapshot {
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

/// One pane holding a terminal ("surface-terminal") and a browser
/// ("surface-browser"), with the BROWSER focused.
fn mixed_surface_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-browser".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("mixed surface layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-mixed".into());
    pane.panel_ids = vec!["surface-terminal".into(), "surface-browser".into()];
    pane.selected_panel_id = Some("surface-browser".into());
    let mut encoded = serde_json::to_value(snapshot).expect("encode mixed fixture");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-terminal", "pane_id": "pane-mixed", "generation": 1,
         "kind": {"type": "terminal"}},
        {"surface_id": "surface-browser", "pane_id": "pane-mixed", "generation": 1,
         "kind": {"type": "browser"}}
    ]);
    serde_json::from_value(encoded).expect("decode mixed fixture")
}

fn browser_only_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.focused_panel_id = Some("surface-browser".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("browser-only layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-browser".into());
    pane.panel_ids = vec!["surface-browser".into()];
    pane.selected_panel_id = Some("surface-browser".into());
    let mut encoded = serde_json::to_value(snapshot).expect("encode browser-only fixture");
    encoded["windows"][0]["tab_manager"]["workspaces"][0]["surfaces"] = json!([
        {"surface_id": "surface-browser", "pane_id": "pane-browser", "generation": 1,
         "kind": {"type": "browser"}}
    ]);
    serde_json::from_value(encoded).expect("decode browser-only fixture")
}

/// window-1 owns a Dock holding one terminal and one browser.
fn dock_snapshot() -> AppSessionSnapshot {
    let snapshot = test_snapshot();
    let mut encoded = serde_json::to_value(snapshot).expect("encode dock fixture");
    encoded["windows"][0]["dock"] = json!({
        "workspace_id": "window-1",
        "layout": {
            "type": "pane",
            "pane": {
                "pane_id": "dock-pane-1",
                "panel_ids": ["dock-terminal", "dock-browser"],
                "selected_panel_id": "dock-terminal"
            }
        },
        "surfaces": [
            {"surface_id": "dock-terminal", "pane_id": "dock-pane-1", "generation": 1,
             "kind": {"type": "terminal"}},
            {"surface_id": "dock-browser", "pane_id": "dock-pane-1", "generation": 1,
             "kind": {"type": "browser"}}
        ],
        "focused_surface_id": "dock-terminal"
    });
    serde_json::from_value(encoded).expect("decode dock fixture")
}

// ---------------------------------------------------------------------------
// Capability advertisement + routing seam
// ---------------------------------------------------------------------------

#[test]
fn window_lifecycle_methods_are_advertised() {
    // v2Capabilities advertisement: TerminalController.swift:2373-2375 (window.*),
    // :2446 (surface.refresh), :2448-2450 (surface.resume.*).
    for method in WINDOW_LIFECYCLE_METHODS {
        assert!(
            CONTROL_SOCKET_METHODS.contains(&method),
            "{method} must be advertised in system.capabilities"
        );
    }
}

#[test]
fn window_lifecycle_methods_route_through_the_lifecycle_seam() {
    for method in WINDOW_LIFECYCLE_METHODS {
        assert_eq!(
            control_request_route_for_method(method),
            ControlRequestRoute::WindowLifecycle,
            "{method} must route through the window-lifecycle transition layer"
        );
    }
    // The already-shipped window reads stay on their existing route.
    assert_eq!(
        control_request_route_for_method("window.list"),
        ControlRequestRoute::Legacy
    );
}

#[test]
fn window_not_found_error_data_is_ref_decorated() {
    // QUIRK: not_found data for window.focus/window.close mints a window_ref
    // for the NONEXISTENT id (ControlCommandCoordinator+Window.swift:129-135,
    // 155-161; ref() mints for ANY uuid, ControlCommandCoordinator.swift:184-187).
    for method in ["window.close", "window.focus"] {
        assert!(error_data_ref_decoration_is_canonical(
            method,
            "not_found",
            "Window not found"
        ));
    }
    assert!(!error_data_ref_decoration_is_canonical(
        "window.create",
        "internal_error",
        "Failed to create window"
    ));
}

// ---------------------------------------------------------------------------
// v2:window.create
// ---------------------------------------------------------------------------

#[test]
fn window_create_ignores_params_entirely() {
    // windowCreate() takes NO params — request.params are ignored entirely
    // (ControlCommandCoordinator+Window.swift:22-23,139).
    let snapshot = test_snapshot();
    let transition = dispatch(
        &snapshot,
        "window.create",
        json!({"window_id": 5, "focus": true, "junk": {"nested": []}}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["window_id"], json!("window-9"));
}

#[test]
fn window_create_success_payload_is_window_id_and_ref_only() {
    let snapshot = test_snapshot();
    let mut transition = dispatch(&snapshot, "window.create", json!({}));
    decorate("window.create", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(sorted_keys(&payload), ["window_id", "window_ref"]);
    assert_eq!(payload["window_id"], json!("window-9"));
    assert_eq!(payload["window_ref"], json!("window:ref:window-9"));
}

#[test]
fn window_create_appends_a_window_with_one_initial_workspace() {
    // createMainWindow: new TabManager with ONE initial workspace
    // (AppDelegate.swift:8639-8645).
    let snapshot = test_snapshot();
    let transition = dispatch(&snapshot, "window.create", json!({}));
    assert!(transition.changed);
    assert_eq!(transition.snapshot.windows.len(), 2);
    let created = &transition.snapshot.windows[1];
    assert_eq!(created.window_id.as_deref(), Some("window-9"));
    assert_eq!(created.tab_manager.workspaces.len(), 1);
    assert_eq!(created.tab_manager.selected_workspace_index, Some(0));
    let workspace = &created.tab_manager.workspaces[0];
    assert_eq!(workspace.focused_panel_id.as_deref(), Some("surface-9"));
    let directory = workspace
        .current_directory
        .as_deref()
        .expect("fresh window workspace must inherit the platform home directory");
    assert!(!directory.trim().is_empty());
    assert_eq!(
        workspace.surfaces.as_deref().unwrap()[0]
            .terminal_startup
            .as_ref()
            .and_then(|startup| startup.working_directory.as_deref()),
        Some(directory)
    );
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_ref() else {
        panic!("created workspace must hold a single pane layout");
    };
    assert_eq!(pane.panel_ids, vec!["surface-9".to_string()]);
}

#[test]
fn window_create_is_order_front_only_and_moves_the_active_pointer() {
    // Socket-created windows must NOT activate/become key: window.create is
    // NOT focus-intent and every socket command suppresses activation
    // (TerminalController.swift:253-275,407-409; AppDelegate.swift:8862-8868).
    // The seam still moves the active TabManager pointer
    // (TerminalControllerControlCommandContext.swift:71-76).
    let snapshot = test_snapshot();
    let transition = dispatch(&snapshot, "window.create", json!({}));
    assert_eq!(
        transition.effects,
        vec![
            WindowLifecycleEffect::WindowCreate {
                window_id: "window-9".into(),
                activate: false,
                failure_code: "internal_error",
                failure_message: "Failed to create window",
            },
            WindowLifecycleEffect::SetActiveWindow {
                window_id: "window-9".into(),
            },
        ],
        "create is orderFront-only (no WindowFocus) and does not persist immediately"
    );
}

#[test]
fn window_create_emits_window_created_with_canonical_payload_keys() {
    // publishCmuxWindowLifecycle payload keys (CmuxLifecycleEventPublishing.swift:281-300);
    // emission at AppDelegate.swift:8860 with origin=create.
    let snapshot = test_snapshot();
    let transition = dispatch(&snapshot, "window.create", json!({}));
    assert_eq!(transition.events.len(), 1);
    let event = &transition.events[0];
    assert_eq!(event.name, "window.created");
    assert_eq!(event.category, "window");
    assert_eq!(event.window_id.as_deref(), Some("window-9"));
    assert_eq!(
        sorted_keys(&event.payload),
        [
            "is_key_window",
            "is_main_window",
            "origin",
            "selected_workspace_index",
            "window_id",
            "workspace_count",
            "workspace_id",
        ]
    );
    assert_eq!(event.payload["origin"], json!("create"));
    assert_eq!(event.payload["window_id"], json!("window-9"));
    assert_eq!(event.payload["workspace_count"], json!(1));
    assert_eq!(event.payload["selected_workspace_index"], json!(0));
    // No key-window transfer: activation is suppressed for socket create.
    assert_eq!(event.payload["is_key_window"], json!(false));
    assert_eq!(event.payload["is_main_window"], json!(false));
}

#[test]
fn repeated_window_create_mints_distinct_ids() {
    let snapshot = test_snapshot();
    let context = WindowLifecycleContext {
        new_window_id: None,
        new_surface_id: None,
        ..test_context()
    };
    let first = dispatch_with_context(&snapshot, "window.create", json!({}), &context);
    let second = dispatch_with_context(&first.snapshot, "window.create", json!({}), &context);
    let first_id = expect_ok(&first.result)["window_id"].clone();
    let second_id = expect_ok(&second.result)["window_id"].clone();
    assert_ne!(first_id, second_id);
    assert_eq!(second.snapshot.windows.len(), 3);
}

// ---------------------------------------------------------------------------
// v2:window.close
// ---------------------------------------------------------------------------

#[test]
fn window_close_rejects_missing_or_invalid_window_id() {
    // invalid_params condition: window_id absent, non-string, whitespace-only,
    // or neither UUID nor resolvable ref (ControlCommandCoordinator+Window.swift:151-153).
    let snapshot = two_window_snapshot();
    for params in [
        json!({}),
        json!({"window_id": null}),
        json!({"window_id": 42}),
        json!({"window_id": "   "}),
        json!({"window_id": "window:9"}), // unresolvable minted-ref shape
    ] {
        let transition = dispatch(&snapshot, "window.close", params.clone());
        let (code, message, data) = expect_error(&transition.result);
        assert_eq!(code, "invalid_params", "{params}");
        assert_eq!(message, "Missing or invalid window_id", "{params}");
        assert_eq!(data, None, "{params}");
        assert!(!transition.changed);
    }
}

#[test]
fn window_close_not_found_mints_a_ref_for_the_nonexistent_id() {
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(
        &snapshot,
        "window.close",
        json!({"window_id": "window-404"}),
    );
    decorate("window.close", &mut transition.result);
    let (code, message, data) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Window not found");
    assert_eq!(
        data,
        Some(json!({
            "window_id": "window-404",
            "window_ref": "window:ref:window-404",
        }))
    );
}

#[test]
fn window_close_success_means_perform_close_invoked() {
    // Success = performClose was INVOKED on an existing window
    // (ControlCommandCoordinator+Window.swift:155-160; AppDelegate.swift:5702-5709).
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    decorate("window.close", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(sorted_keys(&payload), ["window_id", "window_ref"]);
    assert_eq!(payload["window_id"], json!("window-2"));
    assert!(transition.changed);
    assert_eq!(transition.snapshot.windows.len(), 1);
    assert_eq!(
        transition.snapshot.windows[0].window_id.as_deref(),
        Some("window-1")
    );
}

#[test]
fn window_close_runs_the_unregister_sequence_without_focus_mutation() {
    // unregisterMainWindow: history, geometry persist, window.closed publish,
    // notification clearing, repoint, session save
    // (AppDelegate.swift:16241-16305). close_window is not focus-intent: no
    // WindowFocus effect ever.
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    assert_eq!(
        transition.effects,
        vec![
            WindowLifecycleEffect::RecordClosedWindowHistory {
                window_id: "window-2".into(),
            },
            WindowLifecycleEffect::PersistWindowGeometry {
                window_id: "window-2".into(),
            },
            WindowLifecycleEffect::WindowCloseCommit {
                window_id: "window-2".into(),
            },
            WindowLifecycleEffect::ClearWindowNotifications {
                window_id: "window-2".into(),
                workspace_ids: vec!["workspace-2".into()],
            },
            WindowLifecycleEffect::PersistSession,
        ],
        "closing a non-active window does not repoint the active pointer"
    );
}

#[test]
fn window_close_clears_notifications_for_window_and_each_workspace() {
    // Canonical clears forTabId(removed.windowId), then one clear per tab of
    // the removed TabManager, before the active repoint
    // (AppDelegate.swift:16274-16280).
    let mut snapshot = two_window_snapshot();
    let mut extra = snapshot.windows[1].tab_manager.workspaces[0].clone();
    extra.workspace_id = Some("workspace-2b".into());
    extra.focused_panel_id = Some("surface-2b".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        extra.layout.as_mut().expect("extra workspace layout")
    else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2b".into());
    pane.panel_ids = vec!["surface-2b".into()];
    pane.selected_panel_id = Some("surface-2b".into());
    snapshot.windows[1].tab_manager.workspaces.push(extra);

    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    expect_ok(&transition.result);
    let clear = transition
        .effects
        .iter()
        .find(|effect| {
            matches!(
                effect,
                WindowLifecycleEffect::ClearWindowNotifications { .. }
            )
        })
        .expect("close must clear notifications");
    assert_eq!(
        clear,
        &WindowLifecycleEffect::ClearWindowNotifications {
            window_id: "window-2".into(),
            workspace_ids: vec!["workspace-2".into(), "workspace-2b".into()],
        },
        "window id plus every workspace of the closed window, in tab order"
    );
    // The clear runs after the close commit and before the session save
    // (canonical: after publish, before repoint/save).
    let position = |predicate: fn(&WindowLifecycleEffect) -> bool| {
        transition.effects.iter().position(predicate).unwrap()
    };
    assert!(
        position(|effect| matches!(effect, WindowLifecycleEffect::WindowCloseCommit { .. }))
            < position(|effect| matches!(
                effect,
                WindowLifecycleEffect::ClearWindowNotifications { .. }
            ))
    );
    assert!(
        position(|effect| matches!(
            effect,
            WindowLifecycleEffect::ClearWindowNotifications { .. }
        )) < position(|effect| matches!(effect, WindowLifecycleEffect::PersistSession))
    );
}

#[test]
fn last_window_close_does_not_clear_notifications() {
    // The vetoed last-window close leaves the window (and its notifications)
    // alone pending quit confirmation.
    let snapshot = test_snapshot();
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-1"}));
    expect_ok(&transition.result);
    assert!(!transition.effects.iter().any(|effect| matches!(
        effect,
        WindowLifecycleEffect::ClearWindowNotifications { .. }
    )));
}

#[test]
fn window_close_of_the_active_window_repoints_to_first_remaining() {
    // Closing the window that owns the caller's active TabManager repoints to
    // key window else first remaining (AppDelegate.swift:16283-16293).
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-1"}));
    assert!(transition
        .effects
        .contains(&WindowLifecycleEffect::SetActiveWindow {
            window_id: "window-2".into(),
        }));
}

#[test]
fn window_close_emits_window_closed_event() {
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    assert_eq!(transition.events.len(), 1);
    let event = &transition.events[0];
    assert_eq!(event.name, "window.closed");
    assert_eq!(event.category, "window");
    assert_eq!(event.window_id.as_deref(), Some("window-2"));
    assert_eq!(event.payload["origin"], json!("appkit_close"));
    assert_eq!(event.payload["window_id"], json!("window-2"));
    assert_eq!(event.payload["workspace_id"], json!("workspace-2"));
}

#[test]
fn last_window_close_is_success_but_routes_into_quit_confirmation() {
    // Last-window close is app-quit-or-veto, never a plain window close
    // (AppDelegate.swift:16232-16239,12831-12856). The RPC reply is still
    // success ("performClose invoked"), the window stays in the model, and no
    // window.closed is published.
    let snapshot = test_snapshot();
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-1"}));
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["window_id"], json!("window-1"));
    assert!(!transition.changed, "vetoed close must not drop the window");
    assert_eq!(transition.snapshot.windows.len(), 1);
    assert!(transition.events.is_empty());
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::QuitConfirmation {
            window_id: "window-1".into(),
        }]
    );
}

#[test]
fn last_window_close_terminates_when_confirmation_not_required() {
    // handleQuitShortcutWarning: quit confirmation not required ->
    // NSApp.terminate immediately (AppDelegate.swift:12831-12856).
    let snapshot = test_snapshot();
    let context = WindowLifecycleContext {
        quit_confirmation_required: false,
        ..test_context()
    };
    let transition = dispatch_with_context(
        &snapshot,
        "window.close",
        json!({"window_id": "window-1"}),
        &context,
    );
    expect_ok(&transition.result);
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::AppTerminate {
            window_id: "window-1".into(),
        }]
    );
}

#[test]
fn set_active_window_effect_repoints_selectorless_routing() {
    // Canonical defensively repoints the active TabManager after socket
    // window.create (TerminalControllerControlCommandContext.swift:71-76), so
    // subsequent no-window_id commands route to the NEW window (contract
    // adversarial note). The port's pointer is ControlActiveWindowState,
    // written by the executor's SetActiveWindow arm and read by
    // control_active_window_id for selector-less routing.
    let snapshot = two_window_snapshot();
    let create = dispatch(&snapshot, "window.create", json!({}));
    let pointer = ControlActiveWindowState::default();
    assert_eq!(pointer.get(), None, "pointer starts unset");
    for effect in &create.effects {
        if let WindowLifecycleEffect::SetActiveWindow { window_id } = effect {
            pointer.set(window_id);
        }
    }
    assert_eq!(
        pointer.get().as_deref(),
        Some("window-9"),
        "SetActiveWindow must move the stored pointer"
    );
    // A selector-less request routed with the pointer hits the NEW window.
    let current = dispatch_lifecycle_request(
        &create.snapshot,
        "surface.current",
        &serde_json::Map::new(),
        &LifecycleDispatchContext {
            browser_enabled: false,
            dock_available: false,
            active_window_id: pointer.get(),
        },
    );
    let payload = expect_ok(&current.result);
    assert_eq!(payload["window_id"], json!("window-9"));
}

#[test]
fn stored_active_pointer_wins_over_focused_webview_fallback() {
    // Canonical setActiveTabManager overrides the caller default even while
    // another window stays key; the key transition rewrites the pointer via
    // the Focused listener (CmuxLifecycleEventPublishing.swift:258-268), so
    // at read time the stored pointer wins and the focused webview is only
    // the pre-first-write fallback.
    assert_eq!(
        control_active_window_from(Some("window-9".into()), Some("window-1".into())).as_deref(),
        Some("window-9")
    );
    assert_eq!(
        control_active_window_from(None, Some("window-1".into())).as_deref(),
        Some("window-1")
    );
    assert_eq!(control_active_window_from(None, None), None);
}

#[test]
fn startup_fallback_ignores_expired_focus_but_honors_current_focus() {
    let pointer = ControlActiveWindowState::default();
    pointer.set_startup_fallback("window-2");
    pointer.set("window-1");
    assert_eq!(
        pointer.resolve_startup(None).as_deref(),
        Some("window-2"),
        "an expired bootstrap focus must not replace the restored context"
    );

    let pointer = ControlActiveWindowState::default();
    pointer.set_startup_fallback("window-2");
    pointer.set("window-1");
    assert_eq!(
        pointer.resolve_startup(Some("window-1".into())).as_deref(),
        Some("window-1"),
        "a window still focused at first routing must remain authoritative"
    );
}

#[test]
fn session_window_id_for_label_maps_main_to_first_window() {
    let snapshot = two_window_snapshot();
    assert_eq!(
        session_window_id_for_label(&snapshot, "window-2").as_deref(),
        Some("window-2"),
        "aux labels are session ids"
    );
    assert_eq!(
        session_window_id_for_label(&snapshot, "main").as_deref(),
        Some("window-1"),
        "the main webview presents the first session window"
    );
    assert_eq!(session_window_id_for_label(&snapshot, "window-404"), None);
}

#[test]
fn quit_confirmation_setting_defaults_always_and_never_disables() {
    // Canonical QuitConfirmationStore: `app.confirmQuit` mode, default
    // `always`; `never` terminates immediately (QuitConfirmationStore.swift
    // at pinned e1825d40d; handleQuitShortcutWarning AppDelegate.swift:
    // 12831-12856). `dirtyOnly` degrades to always on this port until
    // dirty-workspace tracking exists (documented adaptation). The actual
    // native dialog is live-verify only (canonical bypasses it under XCTest).
    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::app_settings::SettingsStore::new(dir.path().join("settings.json"));
    assert!(
        window_quit_confirmation_required(None),
        "no settings surface -> canonical default (always)"
    );
    assert!(
        window_quit_confirmation_required(Some(&store)),
        "absent key -> canonical default (always)"
    );
    store.set_string(CONFIRM_QUIT_SETTING_KEY, "never");
    assert!(
        !window_quit_confirmation_required(Some(&store)),
        "never -> terminate immediately (AppTerminate effect path)"
    );
    store.set_string(CONFIRM_QUIT_SETTING_KEY, "always");
    assert!(window_quit_confirmation_required(Some(&store)));
    store.set_string(CONFIRM_QUIT_SETTING_KEY, "dirtyOnly");
    assert!(
        window_quit_confirmation_required(Some(&store)),
        "dirtyOnly degrades to always on the port"
    );
    store.set_string(CONFIRM_QUIT_SETTING_KEY, "garbage");
    assert!(
        window_quit_confirmation_required(Some(&store)),
        "unrecognized mode -> canonical default"
    );
}

#[test]
fn repeated_window_close_reports_not_found() {
    let snapshot = two_window_snapshot();
    let first = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    expect_ok(&first.result);
    let second = dispatch(
        &first.snapshot,
        "window.close",
        json!({"window_id": "window-2"}),
    );
    let (code, message, _) = expect_error(&second.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Window not found");
}

#[test]
fn window_close_detaches_remote_tmux_workspaces() {
    // Remote-tmux workspaces detach (server survives) on window close
    // (AppDelegate.swift:8809-8830).
    let snapshot = mark_remote_workspace(two_window_snapshot(), 1);
    let transition = dispatch(&snapshot, "window.close", json!({"window_id": "window-2"}));
    expect_ok(&transition.result);
    assert!(transition
        .effects
        .contains(&WindowLifecycleEffect::RemoteWorkspaceDetach {
            workspace_id: "workspace-2".into(),
            destination: "remote-session-1".into(),
        }));
}

// ---------------------------------------------------------------------------
// v2:window.focus
// ---------------------------------------------------------------------------

#[test]
fn window_focus_rejects_missing_or_invalid_window_id() {
    let snapshot = two_window_snapshot();
    for params in [
        json!({}),
        json!({"window_id": null}),
        json!({"window_id": 42}),
        json!({"window_id": "\n \t"}),
        json!({"window_id": "window:9"}),
    ] {
        let transition = dispatch(&snapshot, "window.focus", params.clone());
        let (code, message, _) = expect_error(&transition.result);
        assert_eq!(code, "invalid_params", "{params}");
        assert_eq!(message, "Missing or invalid window_id", "{params}");
    }
}

#[test]
fn window_focus_not_found_mints_a_ref_for_the_nonexistent_id() {
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(
        &snapshot,
        "window.focus",
        json!({"window_id": "window-404"}),
    );
    decorate("window.focus", &mut transition.result);
    let (code, message, data) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Window not found");
    assert_eq!(
        data,
        Some(json!({
            "window_id": "window-404",
            "window_ref": "window:ref:window-404",
        }))
    );
}

#[test]
fn window_focus_is_the_only_focus_intent_window_method() {
    // window.focus IS in focusIntentV2Methods (TerminalController.swift:253-275):
    // full unhide + deminiaturize + makeKeyAndOrderFront + app activate
    // (MainWindowVisibilityController.swift:134-196), modeled as WindowFocus.
    // v2 window.focus does NOT itself move the active TabManager pointer — it
    // relies on becoming key (divergence from v1 focus_window pinned by the
    // contract's adversarial notes).
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(&snapshot, "window.focus", json!({"window_id": "window-2"}));
    decorate("window.focus", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(sorted_keys(&payload), ["window_id", "window_ref"]);
    assert_eq!(payload["window_id"], json!("window-2"));
    assert!(!transition.changed, "focus is a read of the session model");
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::WindowFocus {
            window_id: "window-2".into(),
        }]
    );
}

#[test]
fn window_focus_emits_window_focused_even_when_already_key() {
    // No key-change guard in focusMainWindow (AppDelegate.swift:5693-5700):
    // focusing an already-key window still publishes window.focused.
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "window.focus", json!({"window_id": "window-1"}));
    expect_ok(&transition.result);
    assert_eq!(transition.events.len(), 1);
    let event = &transition.events[0];
    assert_eq!(event.name, "window.focused");
    assert_eq!(event.payload["origin"], json!("focus_request"));
    assert_eq!(event.payload["is_key_window"], json!(true));
}

// ---------------------------------------------------------------------------
// v2:surface.refresh
// ---------------------------------------------------------------------------

#[test]
fn surface_refresh_unavailable_when_routing_fails() {
    let snapshot = test_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.refresh",
        json!({"window_id": "window-404"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "unavailable");
    assert_eq!(message, "TabManager not available");
}

#[test]
fn surface_refresh_workspace_not_found() {
    let snapshot = test_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.refresh",
        json!({"window_id": "window-1", "workspace_id": "workspace-404"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Workspace not found");
}

#[test]
fn surface_refresh_counts_terminals_only() {
    // A workspace of browsers + 1 terminal returns refreshed=1; browser/other
    // kinds are excluded (ControlCommandCoordinator+Surface2.swift:88-95).
    let snapshot = mixed_surface_snapshot();
    let mut transition = dispatch(&snapshot, "surface.refresh", json!({}));
    decorate("surface.refresh", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(
        sorted_keys(&payload),
        [
            "refreshed",
            "window_id",
            "window_ref",
            "workspace_id",
            "workspace_ref",
        ]
    );
    assert_eq!(payload["refreshed"], json!(1));
    assert_eq!(payload["workspace_id"], json!("workspace-1"));
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::TerminalRefresh {
            surface_id: "surface-terminal".into(),
            reason: "terminalController.v2SurfaceRefresh",
        }]
    );
    assert!(
        transition.events.is_empty(),
        "pure runtime redraw: no event"
    );
    assert!(!transition.changed, "no model mutation");
}

#[test]
fn surface_refresh_of_terminal_free_workspace_is_success_zero() {
    let snapshot = browser_only_snapshot();
    let transition = dispatch(&snapshot, "surface.refresh", json!({}));
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["refreshed"], json!(0));
    assert!(transition.effects.is_empty());
}

#[test]
fn surface_refresh_routes_by_surface_selector() {
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.refresh",
        json!({"surface_id": "surface-2"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["workspace_id"], json!("workspace-2"));
    assert_eq!(payload["refreshed"], json!(1));
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::TerminalRefresh {
            surface_id: "surface-2".into(),
            reason: "terminalController.v2SurfaceRefresh",
        }]
    );
}

#[test]
fn surface_refresh_dock_branch_wins_and_counts_dock_terminals() {
    // Dock branch first: iterate dock panels with reason
    // 'terminalController.v2SurfaceRefresh.windowDock'
    // (TerminalController+ControlSurfaceContext3.swift:79-93).
    let snapshot = dock_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.refresh",
        json!({"workspace_id": "window-1"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["refreshed"], json!(1));
    assert_eq!(payload["window_id"], json!("window-1"));
    assert_eq!(payload["workspace_id"], json!("window-1"));
    assert_eq!(
        transition.effects,
        vec![WindowLifecycleEffect::TerminalRefresh {
            surface_id: "dock-terminal".into(),
            reason: "terminalController.v2SurfaceRefresh.windowDock",
        }]
    );
}

// ---------------------------------------------------------------------------
// v2:surface.resume.set — selector validation + routing + target resolution
// ---------------------------------------------------------------------------

#[test]
fn resume_selector_validation_runs_before_routing_in_fixed_key_order() {
    // For each of window_id, workspace_id, surface_id, terminal_id, tab_id IN
    // THAT ORDER: present-non-null but unresolvable -> invalid_params
    // (surfaceResumeTargetValidationError, ControlCommandCoordinator+Surface3.swift:14-25).
    let snapshot = two_window_snapshot();
    for method in [
        "surface.resume.set",
        "surface.resume.get",
        "surface.resume.clear",
    ] {
        // window_id checked before workspace_id even when both are malformed.
        let transition = dispatch(
            &snapshot,
            method,
            json!({"window_id": 42, "workspace_id": 43, "command": "run"}),
        );
        let (code, message, _) = expect_error(&transition.result);
        assert_eq!(code, "invalid_params", "{method}");
        assert_eq!(message, "Missing or invalid window_id", "{method}");

        // Validation fires even when routing would fail afterwards.
        let transition = dispatch(
            &snapshot,
            method,
            json!({"window_id": "window-404", "workspace_id": 43, "command": "run"}),
        );
        let (_, message, _) = expect_error(&transition.result);
        assert_eq!(message, "Missing or invalid workspace_id", "{method}");
    }
    for (key, params) in [
        (
            "surface_id",
            json!({"surface_id": "surface:9", "command": "run"}),
        ),
        (
            "terminal_id",
            json!({"terminal_id": "  ", "command": "run"}),
        ),
        ("tab_id", json!({"tab_id": 7, "command": "run"})),
    ] {
        let transition = dispatch(&snapshot, "surface.resume.set", params);
        let (code, message, _) = expect_error(&transition.result);
        assert_eq!(code, "invalid_params", "{key}");
        assert_eq!(message, format!("Missing or invalid {key}"), "{key}");
    }
}

#[test]
fn resume_malformed_surface_selector_never_falls_back_to_focused() {
    // Pinned: AppDelegateIssue2907RoutingTests.swift:587-640.
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface:9", "command": "run"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "invalid_params");
    assert_eq!(message, "Missing or invalid surface_id");
    assert!(!transition.changed, "no binding may be stored");
}

#[test]
fn resume_unavailable_uses_the_window_unavailable_message() {
    let snapshot = two_window_snapshot();
    for method in [
        "surface.resume.set",
        "surface.resume.get",
        "surface.resume.clear",
    ] {
        let transition = dispatch(
            &snapshot,
            method,
            json!({"window_id": "window-404", "command": "run"}),
        );
        let (code, message, _) = expect_error(&transition.result);
        assert_eq!(code, "unavailable", "{method}");
        assert_eq!(message, RESUME_UNAVAILABLE, "{method}");
    }
}

#[test]
fn resume_set_missing_command_after_routing() {
    let snapshot = two_window_snapshot();
    for params in [
        json!({}),
        json!({"command": ""}),
        json!({"command": " \n "}),
    ] {
        let transition = dispatch(&snapshot, "surface.resume.set", params.clone());
        let (code, message, _) = expect_error(&transition.result);
        assert_eq!(code, "invalid_params", "{params}");
        assert_eq!(message, "Missing command", "{params}");
    }
    // Routing failure wins over the missing command (error order 2 vs 3).
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"window_id": "window-404"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "unavailable");
    assert_eq!(message, RESUME_UNAVAILABLE);
}

#[test]
fn resume_set_window_scope_restricts_the_search() {
    // Pinned: explicit window_id + surface from another window fails with no
    // mutation (AppDelegateIssue2907RoutingTests.swift:560-585).
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-2", "window_id": "window-1", "command": "run"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Surface not found");
    assert!(!transition.changed);
}

#[test]
fn resume_set_workspace_scope_mismatch_fails_both_workspaces_untouched() {
    // Pinned: AppDelegateIssue2907RoutingTests.swift:1359-1406.
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-1", "workspace_id": "workspace-2", "command": "run"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Surface not found");
    assert!(!transition.changed);
}

#[test]
fn resume_set_locates_explicit_targets_globally_without_scope() {
    // Branch (3): explicit target alone -> GLOBAL AppDelegate.locateSurface
    // across all windows (TerminalController+ControlSurfaceContext4.swift:32-58).
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-2", "command": "run"}),
    );
    decorate("surface.resume.set", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["window_id"], json!("window-2"));
    assert_eq!(payload["workspace_id"], json!("workspace-2"));
    assert_eq!(payload["surface_id"], json!("surface-2"));
}

#[test]
fn resume_set_defaults_to_the_focused_terminal() {
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "surface.resume.set", json!({"command": "run"}));
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["surface_id"], json!("surface-1"));
}

#[test]
fn resume_set_rejects_non_terminal_targets() {
    let snapshot = mixed_surface_snapshot();
    // Focused surface is a browser -> Surface not found.
    let transition = dispatch(&snapshot, "surface.resume.set", json!({"command": "run"}));
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Surface not found");
    // Explicit browser target -> Surface not found.
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-browser", "command": "run"}),
    );
    let (_, message, _) = expect_error(&transition.result);
    assert_eq!(message, "Surface not found");
}

#[test]
fn resume_set_alias_precedence_is_surface_then_terminal_then_tab() {
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-1", "terminal_id": "surface-2", "tab_id": "surface-2",
               "command": "run"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["surface_id"], json!("surface-1"));
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"terminal_id": "surface-2", "tab_id": "surface-1", "command": "run"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["surface_id"], json!("surface-2"));
}

// ---------------------------------------------------------------------------
// v2:surface.resume.set — binding semantics
// ---------------------------------------------------------------------------

#[test]
fn resume_set_stores_and_echoes_the_full_binding_shape() {
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({
            "surface_id": "surface-1",
            "command": "  codex resume abc  ",
            "name": " Agent ",
            "kind": "agent",
            "cwd": " C:/repo ",
            "checkpoint_id": "cp-1",
            "source": "cli",
            "environment": {"FOO": "bar"},
        }),
    );
    decorate("surface.resume.set", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(
        sorted_keys(&payload),
        [
            "cleared",
            "pane_id",
            "pane_ref",
            "resume_binding",
            "surface_id",
            "surface_ref",
            "window_id",
            "window_ref",
            "workspace_id",
            "workspace_ref",
        ]
    );
    assert_eq!(payload["cleared"], json!(false));
    let binding = &payload["resume_binding"];
    assert_eq!(
        sorted_keys(binding),
        [
            "approval_policy",
            "approval_record_id",
            "auto_resume",
            "checkpoint_id",
            "command",
            "cwd",
            "environment",
            "kind",
            "name",
            "source",
            "updated_at",
        ]
    );
    assert_eq!(binding["command"], json!("codex resume abc"));
    assert_eq!(binding["name"], json!("Agent"));
    assert_eq!(binding["kind"], json!("agent"));
    assert_eq!(binding["cwd"], json!("C:/repo"));
    assert_eq!(binding["checkpoint_id"], json!("cp-1"));
    assert_eq!(binding["source"], json!("cli"));
    assert_eq!(binding["environment"], json!({"FOO": "bar"}));
    assert_eq!(binding["auto_resume"], json!(false));
    assert_eq!(binding["approval_policy"], json!(null));
    assert_eq!(binding["approval_record_id"], json!(null));
    assert_eq!(binding["updated_at"], json!(1_700_000_000.5));
    // Persisted in the session model + persistence effect.
    assert!(transition.changed);
    let bindings = transition.snapshot.windows[0].tab_manager.workspaces[0]
        .surface_resume_bindings
        .as_deref()
        .expect("binding stored on the workspace");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].surface_id, "surface-1");
    assert_eq!(bindings[0].binding.command, "codex resume abc");
    assert!(transition
        .effects
        .contains(&WindowLifecycleEffect::PersistSession));
}

#[test]
fn resume_set_get_round_trip_survives_serde() {
    let snapshot = two_window_snapshot();
    let set = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-1", "command": "run", "checkpoint_id": "cp-1"}),
    );
    expect_ok(&set.result);
    let encoded = encode_session(&set.snapshot).expect("encode");
    let decoded = decode_session(&encoded).expect("decode");
    assert_eq!(decoded, set.snapshot, "resume bindings survive persistence");
    let get = dispatch(
        &decoded,
        "surface.resume.get",
        json!({"surface_id": "surface-1"}),
    );
    let payload = expect_ok(&get.result);
    assert_eq!(payload["resume_binding"]["command"], json!("run"));
    assert_eq!(payload["resume_binding"]["checkpoint_id"], json!("cp-1"));
    assert_eq!(payload["cleared"], json!(false));
}

#[test]
fn resume_set_rewrites_process_detected_source_to_manual() {
    // QUIRK: 'process-detected' is rewritten to 'manual' before storage
    // (publicResumeSource, ControlCommandCoordinator+Surface3.swift:28-32).
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "source": "process-detected"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["resume_binding"]["source"], json!("manual"));
}

#[test]
fn resume_set_honors_auto_resume_only_for_agent_hook() {
    // Pinned: 'CannotEnableAutoResumeFromSocket'/'AllowsAgentHookAutoResume'
    // (AppDelegateIssue2907RoutingTests.swift:784-833).
    let snapshot = two_window_snapshot();
    let socket = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "source": "cli", "auto_resume": true}),
    );
    assert_eq!(
        expect_ok(&socket.result)["resume_binding"]["auto_resume"],
        json!(false),
        "auto_resume=true from a non-agent-hook source is silently stored as false"
    );
    let hook = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "source": "agent-hook", "auto_resume": true}),
    );
    assert_eq!(
        expect_ok(&hook.result)["resume_binding"]["auto_resume"],
        json!(true)
    );
}

#[test]
fn resume_set_agent_hook_routes_through_the_approval_effect() {
    // The blocking approval alert is bypassed under tests canonically
    // (TerminalController+ControlSurfaceContext4.swift:104-110); the Windows
    // port pins it as an effect the executor honors.
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "source": "agent-hook", "auto_resume": true}),
    );
    expect_ok(&transition.result);
    assert!(transition
        .effects
        .contains(&WindowLifecycleEffect::ResumeApprovalPrompt {
            surface_id: "surface-1".into(),
            source: Some("agent-hook".into()),
            auto_resume: true,
        }));
}

#[test]
fn resume_set_checkpoint_snake_case_beats_camel_case() {
    // checkpoint_id (snake) then checkpointId (camel) — snake wins
    // (ControlCommandCoordinator+Surface3.swift:56-57).
    let snapshot = two_window_snapshot();
    let both = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "checkpoint_id": "snake", "checkpointId": "camel"}),
    );
    assert_eq!(
        expect_ok(&both.result)["resume_binding"]["checkpoint_id"],
        json!("snake")
    );
    let camel_only = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"command": "run", "checkpointId": "camel"}),
    );
    assert_eq!(
        expect_ok(&camel_only.result)["resume_binding"]["checkpoint_id"],
        json!("camel")
    );
}

// ---------------------------------------------------------------------------
// v2:surface.resume.get
// ---------------------------------------------------------------------------

#[test]
fn resume_get_without_binding_is_success_with_null_binding() {
    // No binding is SUCCESS with resume_binding:null, not not_found.
    let snapshot = two_window_snapshot();
    let mut transition = dispatch(&snapshot, "surface.resume.get", json!({}));
    decorate("surface.resume.get", &mut transition.result);
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["resume_binding"], json!(null));
    assert_eq!(payload["cleared"], json!(false));
    assert_eq!(payload["surface_id"], json!("surface-1"));
    assert_eq!(payload.as_object().unwrap().len(), 10);
    assert!(!transition.changed, "get is read-only");
    assert!(transition.effects.is_empty());
    assert!(transition.events.is_empty());
}

#[test]
fn resume_get_target_not_found() {
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.get",
        json!({"surface_id": "surface-404"}),
    );
    let (code, message, _) = expect_error(&transition.result);
    assert_eq!(code, "not_found");
    assert_eq!(message, "Surface not found");
}

// ---------------------------------------------------------------------------
// v2:surface.resume.clear
// ---------------------------------------------------------------------------

fn snapshot_with_binding() -> AppSessionSnapshot {
    let snapshot = two_window_snapshot();
    let set = dispatch(
        &snapshot,
        "surface.resume.set",
        json!({"surface_id": "surface-1", "command": "run",
               "checkpoint_id": "cp-1", "source": "cli"}),
    );
    expect_ok(&set.result);
    set.snapshot
}

#[test]
fn resume_clear_removes_the_binding() {
    let snapshot = snapshot_with_binding();
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"surface_id": "surface-1"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["cleared"], json!(true));
    assert_eq!(payload["resume_binding"], json!(null));
    assert!(transition.changed);
    assert!(
        transition.snapshot.windows[0].tab_manager.workspaces[0]
            .surface_resume_bindings
            .as_deref()
            .unwrap_or_default()
            .is_empty(),
        "binding removed from the workspace map"
    );
    assert!(transition
        .effects
        .contains(&WindowLifecycleEffect::PersistSession));
}

#[test]
fn resume_clear_of_unbound_surface_still_reports_cleared_true() {
    // clearSurfaceResumeBinding's bool is DISCARDED — clearing an
    // already-clear surface still reports cleared=true
    // (TerminalController+ControlSurfaceContext4.swift:269-288 `_ =`).
    let snapshot = two_window_snapshot();
    let transition = dispatch(&snapshot, "surface.resume.clear", json!({}));
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["cleared"], json!(true));
    assert_eq!(payload["resume_binding"], json!(null));
    assert!(!transition.changed, "nothing to remove: no mutation");
    assert!(
        !transition
            .effects
            .contains(&WindowLifecycleEffect::PersistSession),
        "no mutation, no persistence"
    );
}

#[test]
fn resume_clear_checkpoint_guard_miss_is_success_with_binding_untouched() {
    // Pinned: AppDelegateIssue2907RoutingTests.swift:880-950.
    let snapshot = snapshot_with_binding();
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"surface_id": "surface-1", "checkpoint_id": "cp-2"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["cleared"], json!(false));
    assert_eq!(payload["resume_binding"]["checkpoint_id"], json!("cp-1"));
    assert!(
        !transition.changed,
        "guard miss leaves the binding untouched"
    );
}

#[test]
fn resume_clear_checkpoint_guard_is_evaluated_before_source_guard() {
    let snapshot = snapshot_with_binding();
    // Checkpoint matches, source mismatches -> miss.
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"surface_id": "surface-1", "checkpoint_id": "cp-1", "source": "agent-hook"}),
    );
    assert_eq!(expect_ok(&transition.result)["cleared"], json!(false));
    // Both match -> clear.
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"surface_id": "surface-1", "checkpoint_id": "cp-1", "source": "cli"}),
    );
    assert_eq!(expect_ok(&transition.result)["cleared"], json!(true));
}

#[test]
fn resume_clear_supplied_checkpoint_mismatches_nil_binding() {
    // A supplied checkpoint_id compared against a nil binding mismatches ->
    // cleared=false.
    let snapshot = two_window_snapshot();
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"checkpoint_id": "cp-1"}),
    );
    let payload = expect_ok(&transition.result);
    assert_eq!(payload["cleared"], json!(false));
    assert_eq!(payload["resume_binding"], json!(null));
}

#[test]
fn resume_clear_accepts_camel_checkpoint_alias() {
    let snapshot = snapshot_with_binding();
    let transition = dispatch(
        &snapshot,
        "surface.resume.clear",
        json!({"surface_id": "surface-1", "checkpointId": "cp-1"}),
    );
    assert_eq!(expect_ok(&transition.result)["cleared"], json!(true));
}

// ---------------------------------------------------------------------------
// surface.list renders the stored binding through the ONE shared payload
// builder (contract batch dependency: surfaceResumeBindingPayload must be one
// implementation).
// ---------------------------------------------------------------------------

#[test]
fn surface_list_terminal_rows_render_the_stored_resume_binding() {
    let snapshot = snapshot_with_binding();
    let transition = dispatch_lifecycle_request(
        &snapshot,
        "surface.list",
        &serde_json::Map::new(),
        &LifecycleDispatchContext {
            browser_enabled: false,
            dock_available: false,
            active_window_id: Some("window-1".into()),
        },
    );
    let payload = expect_ok(&transition.result);
    let row = payload["surfaces"]
        .as_array()
        .expect("surface rows")
        .iter()
        .find(|row| row["id"] == json!("surface-1"))
        .expect("surface-1 row");
    assert_eq!(row["resume_binding"]["command"], json!("run"));
    assert_eq!(row["resume_binding"]["checkpoint_id"], json!("cp-1"));
}

#[test]
fn surface_list_rows_carry_the_plain_id_ref_index_keys() {
    // The canonical CLI resolves --window/--surface refs and indexes by
    // reading plain `id`, `ref`, and `index` keys from list rows
    // (CLI/cmux.swift:6096-6111); the `ref` twin is minted by the wrapper's
    // decoration pass (row_is_surface path of decorate_lifecycle_value_refs).
    let snapshot = two_window_snapshot();
    let mut transition = dispatch_lifecycle_request(
        &snapshot,
        "surface.list",
        &serde_json::Map::new(),
        &LifecycleDispatchContext {
            browser_enabled: false,
            dock_available: false,
            active_window_id: Some("window-1".into()),
        },
    );
    decorate_lifecycle_result_refs_with("surface.list", &mut transition.result, &mut |kind, id| {
        format!("{kind}:ref:{id}")
    });
    let payload = expect_ok(&transition.result);
    let row = &payload["surfaces"].as_array().expect("rows")[0];
    assert_eq!(row["id"], json!("surface-1"));
    assert_eq!(row["index"], json!(0));
    assert_eq!(row["ref"], json!("surface:ref:surface-1"));
}

#[test]
fn resume_binding_payload_renders_explicit_nulls() {
    assert_eq!(resume_binding_payload(None), json!(null));
    let binding = cmux_core::session::SessionSurfaceResumeBindingSnapshot {
        name: None,
        kind: None,
        command: "run".into(),
        cwd: None,
        checkpoint_id: None,
        source: None,
        environment: None,
        auto_resume: false,
        approval_policy: None,
        approval_record_id: None,
        updated_at: 42.0,
    };
    assert_eq!(
        resume_binding_payload(Some(&binding)),
        json!({
            "name": null,
            "kind": null,
            "command": "run",
            "cwd": null,
            "checkpoint_id": null,
            "source": null,
            "environment": null,
            "auto_resume": false,
            "approval_policy": null,
            "approval_record_id": null,
            "updated_at": 42.0,
        })
    );
}

// ---------------------------------------------------------------------------
// v1 line protocol: new_window / focus_window / close_window
// ---------------------------------------------------------------------------

#[test]
fn v1_parse_recognizes_exactly_the_three_window_commands() {
    assert_eq!(
        parse_v1_window_command("new_window"),
        Some(V1WindowCommand::NewWindow)
    );
    // Trailing arguments are ignored (no arg validation in the dispatch case,
    // CLI/cmux.swift:4294-4296).
    assert_eq!(
        parse_v1_window_command("new_window extra args"),
        Some(V1WindowCommand::NewWindow)
    );
    assert_eq!(
        parse_v1_window_command("focus_window window-2"),
        Some(V1WindowCommand::FocusWindow(Some("window-2".into())))
    );
    assert_eq!(
        parse_v1_window_command("focus_window"),
        Some(V1WindowCommand::FocusWindow(None))
    );
    assert_eq!(
        parse_v1_window_command("close_window window-2"),
        Some(V1WindowCommand::CloseWindow(Some("window-2".into())))
    );
    assert_eq!(
        parse_v1_window_command("close_window"),
        Some(V1WindowCommand::CloseWindow(None))
    );
    // Anything else falls through to the JSON/v2 pipeline.
    assert_eq!(parse_v1_window_command("list_windows"), None);
    assert_eq!(parse_v1_window_command(""), None);
    assert_eq!(parse_v1_window_command("{\"method\":\"ping\"}"), None);
}

#[test]
fn v1_window_request_maps_to_v2_methods() {
    let (method, params) = v1_window_request(&V1WindowCommand::NewWindow).expect("new_window");
    assert_eq!(method, "window.create");
    assert!(params.is_empty());

    let (method, params) =
        v1_window_request(&V1WindowCommand::FocusWindow(Some("window-2".into())))
            .expect("focus_window");
    assert_eq!(method, "window.focus");
    assert_eq!(params.get("window_id"), Some(&json!("window-2")));

    let (method, params) =
        v1_window_request(&V1WindowCommand::CloseWindow(Some("window-2".into())))
            .expect("close_window");
    assert_eq!(method, "window.close");
    assert_eq!(params.get("window_id"), Some(&json!("window-2")));

    // Server-side arg validation: `ERROR: Invalid window id` for a missing id
    // (TerminalController.swift:11899-11903).
    assert_eq!(
        v1_window_request(&V1WindowCommand::FocusWindow(None)),
        Err("ERROR: Invalid window id".to_string())
    );
    assert_eq!(
        v1_window_request(&V1WindowCommand::CloseWindow(None)),
        Err("ERROR: Invalid window id".to_string())
    );
}

#[test]
fn v1_replies_are_byte_frozen() {
    // new_window success prints `OK <window-uuid>` (TerminalController.swift:11919-11920).
    let snapshot = test_snapshot();
    let create = dispatch(&snapshot, "window.create", json!({}));
    assert_eq!(
        v1_window_reply(&V1WindowCommand::NewWindow, &create.result),
        "OK window-9"
    );
    assert_eq!(
        v1_window_reply(
            &V1WindowCommand::NewWindow,
            &ControlCallResult::Err {
                code: "internal_error".into(),
                message: "Failed to create window".into(),
                data: None,
            }
        ),
        "ERROR: Failed to create window"
    );
    // focus_window/close_window print bare `OK` (TerminalController.swift:11910,11922-11928).
    let two = two_window_snapshot();
    let focus = dispatch(&two, "window.focus", json!({"window_id": "window-2"}));
    assert_eq!(
        v1_window_reply(
            &V1WindowCommand::FocusWindow(Some("window-2".into())),
            &focus.result
        ),
        "OK"
    );
    let close = dispatch(&two, "window.close", json!({"window_id": "window-2"}));
    assert_eq!(
        v1_window_reply(
            &V1WindowCommand::CloseWindow(Some("window-2".into())),
            &close.result
        ),
        "OK"
    );
    // Error mapping: invalid_params -> Invalid window id; not_found -> Window not found.
    let invalid = dispatch(&two, "window.focus", json!({"window_id": "  "}));
    assert_eq!(
        v1_window_reply(
            &V1WindowCommand::FocusWindow(Some("  ".into())),
            &invalid.result
        ),
        "ERROR: Invalid window id"
    );
    let missing = dispatch(&two, "window.focus", json!({"window_id": "window-404"}));
    assert_eq!(
        v1_window_reply(
            &V1WindowCommand::FocusWindow(Some("window-404".into())),
            &missing.result
        ),
        "ERROR: Window not found"
    );
}
