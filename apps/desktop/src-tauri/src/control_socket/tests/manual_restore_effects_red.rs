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

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source.find(signature).expect("function signature");
    let tail = &source[start..];
    let body_start = tail.find('{').expect("function body");
    let mut depth = 0;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function")
}

#[test]
fn additive_restore_does_not_cancel_live_remote_runtime_leases() {
    let source = include_str!("../../control_socket.rs");
    let route = function_source(source, "fn session_restore_previous_launch(");
    assert!(!route.contains("cancel_remote_runtime_leases_for_restore"));
}
