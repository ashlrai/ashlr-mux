#[test]
fn diff_state_exposes_the_registry_unregister_for_owned_starter_compensation() {
    let source = include_str!("../diff.rs");
    let start = source
        .find("pub fn unregister_starter_session(")
        .expect("DiffState must expose starter unregister");
    let tail = &source[start..];
    let end = tail
        .find("\n    }")
        .expect("unregister starter function end");
    let body = &tail[..end];
    assert!(body.contains("self.registry.unregister("));
}
