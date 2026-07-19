use super::*;

#[test]
fn lifecycle_result_decoration_covers_swap_source_target_and_created_ids() {
    let mut value = json!({
        "window_id": "window-current",
        "source_window_id": "window-source",
        "workspace_id": "workspace-current",
        "source_workspace_id": "workspace-source",
        "created_workspace_id": "workspace-created",
        "pane_id": "pane-current",
        "target_pane_id": "pane-target",
        "surface_id": "surface-current",
        "source_surface_id": "surface-source",
        "target_surface_id": "surface-target",
        "created_surface_id": "surface-created",
        "tab_id": "surface-current",
        "created_tab_id": "surface-created",
        "nullable": { "created_surface_id": null },
        "rows": [{ "id": "surface-row" }]
    });
    let mut registry = ControlHandleRegistry::default();
    decorate_lifecycle_value_refs(&mut value, &mut |kind, id| registry.mint(kind, id));

    assert_eq!(value["window_ref"], "window:1");
    assert_eq!(value["source_window_ref"], "window:2");
    assert_eq!(value["workspace_ref"], "workspace:1");
    assert_eq!(value["source_workspace_ref"], "workspace:2");
    assert_eq!(value["created_workspace_ref"], "workspace:3");
    assert_eq!(value["pane_ref"], "pane:1");
    assert_eq!(value["target_pane_ref"], "pane:2");
    assert_eq!(value["surface_ref"], "surface:1");
    assert_eq!(value["source_surface_ref"], "surface:2");
    assert_eq!(value["target_surface_ref"], "surface:3");
    assert_eq!(value["created_surface_ref"], "surface:4");
    assert_eq!(value["tab_ref"], "tab:1");
    assert_eq!(value["created_tab_ref"], "tab:4");
    assert_eq!(value["nullable"]["created_surface_ref"], Value::Null);
    assert_eq!(value["rows"][0]["ref"], "surface:5");
}
