//! RED contracts from the frozen-canonical `browser_core` differential lane.
//!
//! Capture of record: `parity/diff-lane` at `fad714e8a4`, canonical commit
//! `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`, workflow run 29723759002.

use super::*;

#[test]
fn browser_getter_payload_uses_the_canonical_action_envelope() {
    assert_eq!(
        browser_getter_payload("surface-1", BrowserGetter::Text, json!("Hello parity")),
        json!({
            "surface_id": "surface-1",
            "action": "get.text",
            "attempts": 1,
            "value": "Hello parity",
        })
    );

    assert_eq!(
        browser_getter_payload("surface-1", BrowserGetter::Count, json!(3)),
        json!({
            "surface_id": "surface-1",
            "count": 3,
        })
    );
}

#[test]
fn browser_locator_payload_preserves_only_canonical_ref_fields() {
    let value = json!({
        "element_ref": "@e8",
        "selector": "#activate",
        "ref": "e8",
        "tag": "button",
        "text": "Activate parity",
    });

    assert_eq!(
        browser_locator_payload("surface-1", value),
        json!({
            "surface_id": "surface-1",
            "element_ref": "@e8",
            "selector": "#activate",
            "ref": "@e8",
            "tag": "button",
            "text": "Activate parity",
        })
    );
}

#[test]
fn browser_snapshot_script_honors_scope_and_interactive_tree_options() {
    let params = json!({
        "selector": "#snapshot-root",
        "interactive": true,
        "compact": true,
        "max_depth": 3,
    });
    let script = browser_snapshot_script(params.as_object().unwrap());

    assert!(script.contains("const __interactiveOnly = true"));
    assert!(script.contains("const __compact = true"));
    assert!(script.contains("const __maxDepth = 3"));
    assert!(script.contains("const __scopeSelector = \"#snapshot-root\""));
    assert!(script.contains("ready_state"));
    assert!(script.contains("entries"));
}

#[test]
fn browser_text_locator_honors_exact_matching() {
    let params = json!({"text": "Locator needle", "exact": true});
    let script = browser_locator_script(BrowserLocator::Text, params.as_object().unwrap())
        .expect("valid locator");

    assert!(script.contains("const __exact = true"));
    assert!(script.contains("__exact ? (v === __target) : v.includes(__target)"));
}

#[test]
fn browser_type_uses_canonical_trimmed_string_input() {
    let script = browser_action_script(
        BrowserAction::Type,
        Some("#name"),
        " typed",
        "",
        "",
        0.0,
        0.0,
    );

    assert!(script.contains("const __cmuxText = \"typed\";"));
    assert!(!script.contains("const __cmuxText = \" typed\";"));
}
