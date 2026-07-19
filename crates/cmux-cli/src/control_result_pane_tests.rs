#[test]
fn pane_list_outputs_match_canonical_text_rows() {
    assert_eq!(
        format_control_result(
            "pane.list",
            &serde_json::json!({"panes":[
                {"id":"pane-a", "ref":"pane:1", "surface_count":1, "focused":true},
                {"id":"pane-b", "ref":"pane:2", "surface_count":2, "focused":false}
            ]})
        ),
        "* pane:1  [1 surface]  [focused]\n  pane:2  [2 surfaces]"
    );
    assert_eq!(
        format_control_result(
            "pane.surfaces",
            &serde_json::json!({"surfaces":[
                {"id":"surface-a", "ref":"surface:1", "title":"Shell", "selected":true},
                {"id":"surface-b", "ref":"surface:2", "title":"Logs", "selected":false}
            ]})
        ),
        "* surface:1  Shell  [selected]\n  surface:2  Logs"
    );
    assert_eq!(
        format_control_result("pane.list", &serde_json::json!({"panes":[]})),
        "No panes"
    );
    assert_eq!(
        format_control_result("pane.surfaces", &serde_json::json!({"surfaces":[]})),
        "No surfaces in pane"
    );
}

#[test]
fn panel_list_outputs_match_canonical_text_rows() {
    assert_eq!(
        format_control_result(
            "surface.list",
            &serde_json::json!({"surfaces":[
                {"id":"surface-a", "ref":"surface:6", "type":"terminal", "title":"Terminal", "focused":true},
                {"id":"surface-b", "ref":"surface:8", "type":"browser", "title":"Docs", "focused":false}
            ]})
        ),
        "* surface:6  terminal  [focused]  \"Terminal\"\n  surface:8  browser  \"Docs\""
    );
}

#[test]
fn pane_and_panel_focus_include_canonical_scope_handles() {
    let pane = serde_json::json!({
        "pane_ref": "pane:2",
        "workspace_ref": "workspace:3",
    });
    assert_eq!(
        format_control_result("pane.focus", &pane),
        "OK pane:2 workspace:3"
    );

    let panel = serde_json::json!({
        "surface_ref": "surface:4",
        "workspace_ref": "workspace:3",
    });
    assert_eq!(
        format_control_result("surface.focus", &panel),
        "OK surface:4 workspace:3"
    );
}
