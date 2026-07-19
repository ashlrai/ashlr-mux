use super::*;

#[test]
fn pane_surface_responses_use_stable_handles_instead_of_current_indices() {
    let mut mint = |kind: &'static str, id: &str| match (kind, id) {
        ("pane", "pane-created-fifth") => "pane:5".into(),
        ("surface", "surface-created-sixth") => "surface:6".into(),
        unexpected => panic!("unexpected handle request: {unexpected:?}"),
    };

    assert_eq!(
        indexed_response_ref_with("pane", Some("pane-created-fifth"), Some(0), &mut mint,),
        Some("pane:5".into()),
    );
    assert_eq!(
        indexed_response_ref_with("surface", Some("surface-created-sixth"), Some(1), &mut mint,),
        Some("surface:6".into()),
    );
}
