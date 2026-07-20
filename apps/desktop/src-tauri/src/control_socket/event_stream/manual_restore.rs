use super::*;

pub(crate) fn record_manual_restore_window_created(
    app: &AppHandle,
    window: &SessionWindowSnapshot,
) {
    let Some(window_id) = window.window_id.as_deref() else {
        return;
    };
    let event = window_lifecycle::window_lifecycle_event(
        "window.created",
        "create",
        window,
        window_id,
        false,
        false,
    );
    record_event(
        app,
        event.name,
        event.category,
        event.source,
        event.window_id,
        event.workspace_id,
        event.pane_id,
        event.surface_id,
        event.payload,
    );
}
