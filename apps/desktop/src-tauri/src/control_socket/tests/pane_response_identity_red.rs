use super::*;

#[test]
fn pane_responses_use_session_window_identity_without_a_native_match() {
    let snapshot = test_snapshot();

    assert_eq!(
        pane_response_window_identity(&snapshot.windows[0], 0),
        (Some("window-1".into()), Some("window:1".into()))
    );
}
