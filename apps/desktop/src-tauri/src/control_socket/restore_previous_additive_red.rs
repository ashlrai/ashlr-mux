//! Decoded control-plane contract for additive previous-session restore.
//!
//! The hosted WebView assertion is deliberately ignored locally and remains a
//! failing test when explicitly run. Model, response, event, and focus policy
//! stay deterministic in the normal unit-test process.

use super::*;

fn id(value: u128) -> String {
    Uuid::from_u128(value).to_string()
}

fn socket_snapshot(seed: u128) -> AppSessionSnapshot {
    let window_id = id(seed);
    let workspace_id = id(seed + 0x1000);
    let pane_id = id(seed + 0x2000);
    let surface_id = id(seed + 0x3000);
    let mut workspace = session_ops::fresh_terminal_workspace(&surface_id);
    workspace.workspace_id = Some(workspace_id.clone());
    workspace.process_title = format!("workspace-{seed:x}");
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() else {
        panic!("single-pane fixture")
    };
    pane.pane_id = Some(pane_id.clone());
    pane.selected_panel_id = Some(surface_id.clone());
    workspace.surfaces = Some(vec![cmux_core::session::SessionSurfaceSnapshot {
        surface_id: surface_id.clone(),
        pane_id,
        generation: 1,
        kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
        metadata: Default::default(),
        terminal_startup: None,
    }]);
    AppSessionSnapshot {
        version: cmux_core::session::SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 1,
        windows: vec![cmux_core::session::SessionWindowSnapshot {
            window_id: Some(window_id),
            selected_workspace_id: Some(workspace_id),
            dock: None,
            tab_manager: cmux_core::session::SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace],
                workspace_groups: None,
            },
        }],
    }
}

fn current_decoded_restore_response(
    current: &AppSessionSnapshot,
    previous: Option<&AppSessionSnapshot>,
) -> ControlCallResult {
    workspace_current(previous.unwrap_or(current))
}

fn decoded_ok(result: ControlCallResult) -> Value {
    match result {
        ControlCallResult::Ok(value) => Value::from(value),
        ControlCallResult::Err { code, message, .. } => {
            panic!("expected ok response, got {code}: {message}")
        }
    }
}

#[test]
fn decoded_success_payload_is_only_restored_true() {
    let current = socket_snapshot(0x100);
    let previous = socket_snapshot(0x200);

    let payload = decoded_ok(current_decoded_restore_response(&current, Some(&previous)));

    assert!(
        payload == json!({"restored": true}),
        "restore response was not the canonical one-field acknowledgement"
    );
}

#[test]
fn decoded_missing_or_unusable_previous_is_exact_not_found() {
    let current = socket_snapshot(0x300);

    let response = current_decoded_restore_response(&current, None);

    assert!(
        response
            == ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "No previous session snapshot available".to_string(),
                data: None,
            },
        "missing previous snapshot did not decode as canonical not_found"
    );
}

#[test]
fn additive_batch_records_only_explicit_window_created_lifecycle_events() {
    let current = socket_snapshot(0x400);
    let mut combined = current.clone();
    combined
        .windows
        .push(socket_snapshot(0x500).windows.remove(0));
    let before = session_event_summaries(&current);
    let after = session_event_summaries(&combined);
    let new_window_id = combined.windows[1].window_id.as_ref().expect("window id");
    assert!(!before.contains_key(new_window_id));
    let summary = after.get(new_window_id).expect("new window summary");

    let names = derived_session_event_specs(None, summary)
        .into_iter()
        .map(|event| event.name)
        .collect::<Vec<_>>();

    assert_eq!(names, ["window.created"]);
}

#[test]
fn socket_restore_has_no_focus_or_selection_event_intent() {
    let restored = socket_snapshot(0x600);
    let summary = session_event_summaries(&restored)
        .into_values()
        .next()
        .expect("window summary");

    let names = derived_session_event_specs(None, &summary)
        .into_iter()
        .map(|event| event.name)
        .collect::<Vec<_>>();

    assert!(!names.iter().any(|name| {
        name.ends_with(".focused") || name.ends_with(".selected") || *name == "window.keyed"
    }));
}

#[test]
#[ignore = "HOSTED LIVE RED: requires a real Tauri runtime with main plus restored WebViews"]
fn hosted_live_restore_creates_real_unfocused_webviews_with_window_local_projection() {
    panic!(
        "HOSTED LIVE RED: invoke session.restore_previous against a running app; assert every \
         returned restored UUID has a real WebView, existing focus is unchanged, and each restored \
         label receives a session projection with its own window first"
    );
}
