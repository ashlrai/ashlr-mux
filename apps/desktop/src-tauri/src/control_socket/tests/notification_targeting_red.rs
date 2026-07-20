use super::*;

const WORKSPACE_A: &str = "10000000-0000-4000-8000-000000000001";
const SURFACE_A: &str = "20000000-0000-4000-8000-000000000001";
const WORKSPACE_B: &str = "10000000-0000-4000-8000-000000000002";
const SURFACE_B: &str = "20000000-0000-4000-8000-000000000002";

fn targeting_snapshot() -> AppSessionSnapshot {
    let mut snapshot = test_snapshot();
    let first = &mut snapshot.windows[0].tab_manager.workspaces[0];
    first.workspace_id = Some(WORKSPACE_A.into());
    first.focused_panel_id = Some(SURFACE_A.into());
    first.panel_ttys = Some(vec![SessionPanelTtySnapshot {
        panel_id: SURFACE_A.into(),
        tty: "/dev/ttys111".into(),
        updated_at: 1,
    }]);
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        first.layout.as_mut().expect("first workspace layout")
    else {
        unreachable!();
    };
    pane.panel_ids = vec![SURFACE_A.into()];
    pane.selected_panel_id = Some(SURFACE_A.into());

    let mut second = first.clone();
    second.workspace_id = Some(WORKSPACE_B.into());
    second.focused_panel_id = Some(SURFACE_B.into());
    second.panel_ttys = Some(vec![SessionPanelTtySnapshot {
        panel_id: SURFACE_B.into(),
        tty: "/dev/ttys222".into(),
        updated_at: 2,
    }]);
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        second.layout.as_mut().expect("second workspace layout")
    else {
        unreachable!();
    };
    pane.panel_ids = vec![SURFACE_B.into()];
    pane.selected_panel_id = Some(SURFACE_B.into());
    snapshot.windows[0].tab_manager.workspaces.push(second);
    snapshot
}

#[test]
fn specialized_notification_methods_are_desktop_capabilities_only() {
    for method in [
        "notification.create_for_caller",
        "notification.create_for_surface",
        "notification.create_for_target",
    ] {
        assert!(CONTROL_SOCKET_METHODS.contains(&method), "missing {method}");
    }
    assert!(
        !CONTROL_SOCKET_METHODS.contains(&"notification.reconcile"),
        "canonical reconcile belongs to the authenticated mobile-host RPC"
    );
}

#[test]
fn caller_target_precedence_matches_canonical_tty_workspace_surface_fallbacks() {
    let snapshot = targeting_snapshot();

    let tty_first = resolve_caller_notification_target(
        &snapshot,
        Some(WORKSPACE_A),
        Some(SURFACE_A),
        Some("ttys222"),
        true,
    )
    .expect("TTY target");
    assert_eq!(tty_first.workspace_id, WORKSPACE_B);
    assert_eq!(tty_first.surface_id, SURFACE_B);

    let workspace_first = resolve_caller_notification_target(
        &snapshot,
        Some(WORKSPACE_A),
        Some("30000000-0000-4000-8000-000000000099"),
        Some("ttys222"),
        false,
    )
    .expect("preferred workspace target");
    assert_eq!(workspace_first.workspace_id, WORKSPACE_A);
    assert_eq!(workspace_first.surface_id, SURFACE_A);

    let stale_workspace_surface = resolve_caller_notification_target(
        &snapshot,
        Some("30000000-0000-4000-8000-000000000098"),
        Some(SURFACE_B),
        None,
        false,
    )
    .expect("preferred surface target");
    assert_eq!(stale_workspace_surface.workspace_id, WORKSPACE_B);
    assert_eq!(stale_workspace_surface.surface_id, SURFACE_B);

    let selected = resolve_caller_notification_target(&snapshot, None, None, None, false)
        .expect("selected fallback");
    assert_eq!(selected.workspace_id, WORKSPACE_A);
    assert_eq!(selected.surface_id, SURFACE_A);
}

#[test]
fn caller_tty_normalization_is_byte_faithful_to_canonical() {
    assert_eq!(
        normalized_notification_tty(Some(" /dev/ttys777\n")),
        Some("ttys777")
    );
    assert_eq!(normalized_notification_tty(Some("pts/4")), Some("4"));
    assert_eq!(normalized_notification_tty(Some("not a tty")), None);
    assert_eq!(normalized_notification_tty(Some(" \t")), None);
    assert_eq!(normalized_notification_tty(None), None);
}

#[test]
fn specialized_request_events_preserve_the_resolved_window() {
    let requested = notification_v2_request_event_spec(
        "notification.requested",
        "notification.create_for_target",
        json!({"workspace_id": WORKSPACE_A, "surface_id": SURFACE_A}),
        json!({
            "workspace_id": WORKSPACE_A,
            "surface_id": SURFACE_A,
            "window_id": "30000000-0000-4000-8000-000000000001",
        }),
        Some(WORKSPACE_A),
        Some(SURFACE_A),
    );

    assert_eq!(
        requested.window_id.as_deref(),
        Some("30000000-0000-4000-8000-000000000001")
    );
}
