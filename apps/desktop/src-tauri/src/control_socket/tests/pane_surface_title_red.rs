use super::*;

#[test]
fn pane_surface_default_titles_match_workspace_anchor_and_terminal_labels() {
    let mut snapshot = test_snapshot();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.custom_title = Some("pane-management".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    pane.panel_ids.push("surface-2".into());

    assert_eq!(
        pane_surface_control::pane_surfaces::pane_surface_default_title(
            workspace,
            "surface-1",
            "terminal",
        ),
        "pane-management",
    );
    assert_eq!(
        pane_surface_control::pane_surfaces::pane_surface_default_title(
            workspace,
            "surface-2",
            "terminal",
        ),
        "Terminal",
    );
}
