use super::*;

/// The canonical bonsplit selection pair: surface.selected (with
/// previous_surface_id) followed by surface.focused, origin
/// "bonsplit_selection" (live capture surface_create.terminal_happy /
/// surface_close.happy frames).
#[allow(clippy::too_many_arguments)]
pub(super) fn selection_events(
    window_id: &str,
    workspace_id: &str,
    pane_id: &str,
    surface_id: &str,
    previous_surface_id: Option<&str>,
    kind: &str,
    focused: bool,
) -> [LifecycleEvent; 2] {
    [
        owned_event(
            "surface.selected",
            window_id,
            workspace_id,
            Some(pane_id),
            Some(surface_id),
            json!({
                "focused": focused,
                "kind": kind,
                "origin": "bonsplit_selection",
                "pane_id": pane_id,
                "previous_surface_id": previous_surface_id,
                "surface_id": surface_id,
            }),
        ),
        owned_event(
            "surface.focused",
            window_id,
            workspace_id,
            Some(pane_id),
            Some(surface_id),
            json!({
                "kind": kind,
                "origin": "bonsplit_selection",
                "pane_id": pane_id,
                "surface_id": surface_id,
            }),
        ),
    ]
}

pub(super) fn focus_selection_events(
    window_id: &str,
    workspace_id: &str,
    pane_id: &str,
    surface_id: &str,
    kind: &str,
) -> [LifecycleEvent; 3] {
    let [selected, _] = selection_events(
        window_id,
        workspace_id,
        pane_id,
        surface_id,
        None,
        kind,
        true,
    );
    let [pane_focused, focused] =
        focused_pane_events(window_id, workspace_id, pane_id, surface_id, kind);
    [selected, pane_focused, focused]
}

pub(super) fn focused_pane_events(
    window_id: &str,
    workspace_id: &str,
    pane_id: &str,
    surface_id: &str,
    kind: &str,
) -> [LifecycleEvent; 2] {
    [
        owned_event(
            "pane.focused",
            window_id,
            workspace_id,
            Some(pane_id),
            Some(surface_id),
            json!({
                "origin": "bonsplit_selection",
                "pane_id": pane_id,
                "selected_surface_id": surface_id,
            }),
        ),
        owned_event(
            "surface.focused",
            window_id,
            workspace_id,
            Some(pane_id),
            Some(surface_id),
            json!({
                "kind": kind,
                "origin": "bonsplit_selection",
                "pane_id": pane_id,
                "surface_id": surface_id,
            }),
        ),
    ]
}
