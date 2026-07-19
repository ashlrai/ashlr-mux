use super::pane_surface_lifecycle::{
    dispatch_lifecycle_request, LifecycleDispatchContext, LifecycleTransition,
};
use super::pane_surface_lifecycle_red::mixed_surface_snapshot;
use super::*;
use serde_json::{json, Value};

fn transition(snapshot: &AppSessionSnapshot, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(
        snapshot,
        "surface.focus",
        params.as_object().expect("decoded params object"),
        &LifecycleDispatchContext {
            browser_enabled: true,
            dock_available: true,
            active_window_id: None,
        },
    )
}

#[test]
fn cli_focus_panel_publishes_focus_then_tab_selection() {
    let focused = transition(
        &mixed_surface_snapshot(),
        json!({
            "__cmux_cli_command": "focus-panel",
            "surface_id": "surface-terminal",
        }),
    );

    assert_eq!(
        focused
            .events
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>(),
        [
            "window.focused",
            "pane.focused",
            "surface.focused",
            "surface.selected",
            "surface.focused",
        ]
    );
    assert_eq!(
        focused.events[1].surface_id.as_deref(),
        Some("surface-browser")
    );
    assert_eq!(
        focused.events[2].surface_id.as_deref(),
        Some("surface-browser")
    );
    assert_eq!(
        focused.events[3].surface_id.as_deref(),
        Some("surface-terminal")
    );
    assert_eq!(
        focused.events[3].payload["previous_surface_id"],
        json!("surface-browser")
    );
    assert_eq!(
        focused.events[4].surface_id.as_deref(),
        Some("surface-terminal")
    );
}
