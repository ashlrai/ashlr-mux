use super::*;

#[test]
fn grid_metrics_follow_the_authoritative_selected_surface_kind() {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.surfaces = Some(vec![cmux_core::session::SessionSurfaceSnapshot {
        surface_id: "surface-1".to_string(),
        pane_id: "pane-1".to_string(),
        generation: 1,
        kind: SessionSurfaceKindSnapshot::Browser {
            url: Some("https://example.test".to_string()),
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
        metadata: Default::default(),
        terminal_startup: None,
        scrollback: None,
    }]);
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_ref().expect("pane layout")
    else {
        unreachable!();
    };
    assert_eq!(pane.surface_kind, None, "fixture exercises the v2 model");
    assert!(
        !pane_surface_control::pane_list::pane_list_selected_surface_is_terminal(
            workspace,
            pane,
            Some("surface-1"),
        )
    );
}
