use super::*;

#[test]
fn pane_list_uses_stable_creation_refs_instead_of_current_visual_indices() {
    let snapshot = test_snapshot();
    let SessionWorkspaceLayoutSnapshot::Pane(mut pane) = snapshot.windows[0].tab_manager.workspaces
        [0]
    .layout
    .clone()
    .expect("fixture layout") else {
        unreachable!()
    };
    pane.pane_id = Some("pane-created-fifth".into());
    pane.panel_ids = vec![
        "surface-created-sixth".into(),
        "surface-created-third".into(),
    ];
    pane.selected_panel_id = Some("surface-created-sixth".into());
    let mut mint = |kind: &'static str, id: &str| match (kind, id) {
        ("pane", "pane-created-fifth") => "pane:5".into(),
        ("surface", "surface-created-sixth") => "surface:6".into(),
        ("surface", "surface-created-third") => "surface:3".into(),
        unexpected => panic!("unexpected handle request: {unexpected:?}"),
    };

    assert_eq!(
        pane_surface_control::pane_list::pane_list_reference_fields_with(
            &pane,
            0,
            pane.selected_panel_id.as_deref(),
            &mut mint,
        ),
        (
            "pane:5".into(),
            vec!["surface:6".into(), "surface:3".into()],
            Some("surface:6".into()),
        )
    );
}

#[test]
fn pane_list_resolves_the_first_session_window_through_the_main_webview() {
    let snapshot = test_snapshot();
    let window_id = snapshot.windows[0]
        .window_id
        .as_deref()
        .expect("fixture window id");
    let mut requested_labels = Vec::new();

    let size = pane_surface_control::pane_list::pane_list_window_size_with(
        &snapshot,
        window_id,
        |label| {
            requested_labels.push(label.to_owned());
            (label == "main").then_some((1000.0, 700.0))
        },
    );

    assert_eq!(requested_labels, ["main"]);
    assert_eq!(size, (1000.0, 700.0));

    requested_labels.clear();
    let auxiliary_size = pane_surface_control::pane_list::pane_list_window_size_with(
        &snapshot,
        "window-2",
        |label| {
            requested_labels.push(label.to_owned());
            (label == "window-2").then_some((800.0, 600.0))
        },
    );
    assert_eq!(requested_labels, ["window-2"]);
    assert_eq!(auxiliary_size, (800.0, 600.0));
}

#[test]
fn pane_list_reports_logical_points_instead_of_physical_dpi_pixels() {
    assert_eq!(
        pane_surface_control::pane_list::pane_list_logical_size(1600, 1050, 1.25),
        (1280.0, 840.0)
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_logical_size(800, 600, f64::NAN),
        (800.0, 600.0)
    );
}

#[test]
fn pane_list_prefers_the_rendered_workspace_frame_over_the_native_window() {
    let observed = PanePixelFrame {
        x: 240.0,
        y: 28.0,
        width: 760.0,
        height: 672.0,
    };
    let native_window = PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 700.0,
    };

    assert_eq!(
        pane_surface_control::pane_list::pane_list_root_frame(Some(observed), native_window),
        observed
    );
}

#[test]
fn pane_list_container_size_uses_the_rendered_workspace_dimensions() {
    let rendered = PanePixelFrame {
        x: 240.0,
        y: 28.0,
        width: 760.0,
        height: 672.0,
    };

    assert_eq!(
        pane_surface_control::pane_list::pane_list_container_size(rendered, 1000.0, 700.0),
        (760.0, 672.0)
    );
}
