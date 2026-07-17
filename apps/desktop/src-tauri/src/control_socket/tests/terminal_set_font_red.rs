use super::*;
use terminal_runtime_v2::plan_terminal_set_font_request;

fn params(value: Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().expect("object params")
}

#[test]
fn set_font_is_mobile_only_and_public_exactly_once() {
    assert_eq!(
        CONTROL_SOCKET_METHODS
            .iter()
            .filter(|method| **method == "mobile.terminal.set_font")
            .count(),
        1
    );
    assert!(!CONTROL_SOCKET_METHODS.contains(&"terminal.set_font"));
}

#[test]
fn set_font_accepts_canonical_numbers_strings_and_booleans() {
    for (raw, expected) in [
        (json!(12.5), 12.5),
        (json!("13.75"), 13.75),
        (json!(true), 1.0),
    ] {
        let plan = plan_terminal_set_font_request(&params(json!({"font_size": raw})))
            .expect("valid canonical font size");
        assert_eq!(plan.font_size, expected);
        assert_eq!(plan.surface_id, None);
        assert_eq!(plan.workspace_id, None);
    }
}

#[test]
fn set_font_preserves_only_raw_string_scopes() {
    let plan = plan_terminal_set_font_request(&params(json!({
        "font_size": 14,
        "surface_id": "  opaque surface  ",
        "workspace_id": "",
    })))
    .expect("raw string scopes");
    assert_eq!(plan.surface_id.as_deref(), Some("  opaque surface  "));
    assert_eq!(plan.workspace_id.as_deref(), Some(""));

    let plan = plan_terminal_set_font_request(&params(json!({
        "font_size": 14,
        "surface_id": 7,
        "workspace_id": false,
    })))
    .expect("non-string scopes are ignored");
    assert_eq!(plan.surface_id, None);
    assert_eq!(plan.workspace_id, None);
}

#[test]
fn set_font_freezes_exact_validation_classes() {
    for invalid in [
        json!({}),
        json!({"font_size": null}),
        json!({"font_size": "large"}),
    ] {
        let error =
            plan_terminal_set_font_request(&params(invalid)).expect_err("invalid font size");
        assert_eq!(error.code, "invalid_params");
        assert_eq!(error.message, "Missing or invalid font_size");
    }

    for invalid in [
        json!({"font_size": false}),
        json!({"font_size": 0}),
        json!({"font_size": -2.5}),
        json!({"font_size": "NaN"}),
        json!({"font_size": "inf"}),
    ] {
        let error = plan_terminal_set_font_request(&params(invalid)).expect_err("nonpositive size");
        assert_eq!(error.code, "invalid_params");
        assert_eq!(
            error.message,
            "font_size must be a positive number of points"
        );
    }
}

#[test]
fn delivered_reflects_pre_emit_matching_subscription_presence() {
    let event = json!({"name": "terminal.set_font", "category": "terminal"});
    let (matching_sender, _matching_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let (other_sender, _other_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let subscribers = vec![
        EventSubscriber {
            sender: matching_sender,
            names: vec!["terminal.set_font".into()],
            categories: Vec::new(),
        },
        EventSubscriber {
            sender: other_sender,
            names: vec!["workspace.created".into()],
            categories: vec!["workspace".into()],
        },
    ];
    assert!(event_subscribers_match(&subscribers, &event));

    let unrelated = json!({"name": "terminal.updated", "category": "terminal"});
    assert!(!event_subscribers_match(&subscribers, &unrelated));
}
