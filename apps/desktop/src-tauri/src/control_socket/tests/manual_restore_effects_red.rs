//! Canonical socket contract for manual previous-launch restore.

use super::*;

fn outcome(restored: bool) -> RestorePreviousLaunchOutcome {
    RestorePreviousLaunchOutcome {
        snapshot: test_snapshot(),
        restored,
    }
}

#[test]
fn restored_outcome_maps_to_exact_canonical_success_payload() {
    let ControlCallResult::Ok(value) = session_restore_previous_result(&outcome(true)) else {
        panic!("successful restore must return ok")
    };

    assert_eq!(Value::from(value), json!({ "restored": true }));
}

#[test]
fn unavailable_outcome_maps_to_exact_canonical_not_found() {
    let ControlCallResult::Err {
        code,
        message,
        data,
    } = session_restore_previous_result(&outcome(false))
    else {
        panic!("missing previous snapshot must return not_found")
    };

    assert_eq!(code, "not_found");
    assert_eq!(message, "No previous session snapshot available");
    assert_eq!(data, None);
}
