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
    use crate::pane_geometry::PaneGeometryAuthority;
    use std::cell::Cell;

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

    let native_reads = Cell::new(0);
    assert_eq!(
        pane_surface_control::pane_list::pane_list_root_frame(
            PaneGeometryAuthority::Rendered(crate::pane_geometry::WorkspacePaneGeometry {
                x: observed.x,
                y: observed.y,
                width: observed.width,
                height: observed.height,
            }),
            None,
            || {
                native_reads.set(native_reads.get() + 1);
                native_window
            },
        ),
        observed
    );
    assert_eq!(
        native_reads.get(),
        0,
        "rendered geometry must not wait on native window state"
    );
}

#[test]
fn pane_list_uses_zero_frames_only_after_the_window_has_rendered_another_workspace() {
    use crate::pane_geometry::PaneGeometryAuthority;
    use std::cell::Cell;

    let native_window = PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 700.0,
    };
    let zero = PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    let native_reads = Cell::new(0);
    assert_eq!(
        pane_surface_control::pane_list::pane_list_root_frame(
            PaneGeometryAuthority::Uninitialized,
            None,
            || {
                native_reads.set(native_reads.get() + 1);
                native_window
            },
        ),
        native_window
    );
    assert_eq!(native_reads.get(), 1);
    assert_eq!(
        pane_surface_control::pane_list::pane_list_root_frame(
            PaneGeometryAuthority::WorkspaceUnrendered,
            None,
            || {
                native_reads.set(native_reads.get() + 1);
                native_window
            },
        ),
        zero
    );
    assert_eq!(
        native_reads.get(),
        1,
        "unrendered workspace authority must not read native window state"
    );
}

#[test]
fn pane_list_headless_capture_does_not_invent_geometry_before_a_workspace_renders() {
    use crate::pane_geometry::PaneGeometryAuthority;
    use std::cell::Cell;

    let configured = PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 700.0,
    };
    let zero = PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    let native_reads = Cell::new(0);

    assert_eq!(
        pane_surface_control::pane_list::pane_list_root_frame(
            PaneGeometryAuthority::Uninitialized,
            Some(configured),
            || {
                native_reads.set(native_reads.get() + 1);
                configured
            },
        ),
        zero
    );
    assert_eq!(
        native_reads.get(),
        0,
        "headless capture must not enter the native window actor"
    );
}

#[test]
fn pane_list_projects_canonical_grid_metrics_while_terminal_resize_catches_up() {
    let root = PanePixelFrame {
        x: 240.0,
        y: 28.0,
        width: 760.0,
        height: 672.0,
    };

    assert_eq!(
        pane_surface_control::pane_list::pane_list_provisional_grid_fields(
            PanePixelFrame {
                x: 240.0,
                y: 28.0,
                width: 190.0,
                height: 336.0,
            },
            root,
        ),
        Some((23, 17, 8, 17))
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_provisional_grid_fields(
            PanePixelFrame {
                x: 620.0,
                y: 28.0,
                width: 380.0,
                height: 672.0,
            },
            root,
        ),
        Some((46, 37, 8, 17)),
        "the outer terminal scrollbar consumes one cell at the right edge"
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_provisional_grid_fields(
            PanePixelFrame {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            root,
        ),
        None,
        "an unrendered workspace must not fabricate live terminal metrics"
    );
}

#[test]
fn pane_list_headless_selection_uses_the_configured_workspace_portal() {
    assert_eq!(
        pane_surface_control::pane_list::pane_list_capture_portal_frame(1000.0, 700.0),
        Some(PanePixelFrame {
            x: 240.0,
            y: 28.0,
            width: 760.0,
            height: 672.0,
        })
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_capture_portal_frame(200.0, 20.0),
        None
    );
}

#[test]
fn pane_list_prefers_fresh_projection_and_retains_it_for_an_unrendered_move() {
    let projected = Some((23, 17, 8, 17));
    let retained = Some((47, 17, 8, 17));
    let stale_retained = Some((80, 20, 8, 16));

    assert_eq!(
        pane_surface_control::pane_list::pane_list_preferred_grid_fields(
            projected,
            stale_retained,
            false,
        ),
        projected
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_preferred_grid_fields(None, retained, false),
        retained,
        "a moved live pane keeps its last rendered grid while its new workspace is hidden"
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_preferred_grid_fields(None, None, false),
        None,
        "an unrendered workspace must not expose an unconfirmed default grid"
    );
    assert_eq!(
        pane_surface_control::pane_list::pane_list_preferred_grid_fields(
            projected,
            stale_retained,
            true,
        ),
        None,
        "the first activation read exposes geometry before its terminal grid is live"
    );
}

#[test]
fn pane_list_suppresses_grid_on_first_capture_activation_despite_other_workspace_geometry() {
    assert!(
        pane_surface_control::pane_list::pane_list_capture_activation_bootstrap(true, false, true,),
        "another workspace's latest frame must not make this workspace look rendered"
    );
    assert!(
        !pane_surface_control::pane_list::pane_list_capture_activation_bootstrap(true, true, true,),
        "a workspace with its own rendered geometry is no longer bootstrapping"
    );
}

#[test]
fn pane_grid_carryover_is_shared_across_runtime_handoffs_and_pruned_on_close() {
    use crate::pane_geometry::PaneGeometryState;
    use std::collections::HashSet;

    let state = PaneGeometryState::default();
    state.remember_grid_fields("surface-1", (23, 17, 8, 17));
    assert_eq!(
        state.grid_fields_for_panel("surface-1"),
        Some((23, 17, 8, 17))
    );

    state.retain_grid_fields(&HashSet::from(["surface-2".to_string()]));
    assert_eq!(state.grid_fields_for_panel("surface-1"), None);
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
        pane_surface_control::pane_list::pane_list_container_size(rendered),
        (760.0, 672.0)
    );
}
