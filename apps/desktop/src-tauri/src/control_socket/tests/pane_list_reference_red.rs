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
        pane_list_reference_fields_with(
            &pane,
            0,
            &[
                "surface-created-third".into(),
                "surface-created-sixth".into()
            ],
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
