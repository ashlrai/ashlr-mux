use super::*;

pub(super) fn notify_session_changed_with_event_policy(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    event_policy: DerivedEventPolicy,
) {
    if let Some(state) = app.try_state::<SessionState>() {
        record_workspace_focus_history(state.inner(), snapshot);
    }
    let _ = persist_current_snapshot(app, snapshot);
    match event_policy {
        DerivedEventPolicy::Record => {
            crate::control_socket::record_session_changed_event(app, snapshot)
        }
        DerivedEventPolicy::Suppress => {
            crate::control_socket::replace_session_event_baseline(app, snapshot)
        }
    }
    emit_session_changed(app, snapshot);
    crate::window_title::refresh_window_titles(app, snapshot);
    crate::window::emit_window_states(app);
}

/// Atomically publish one already-validated application-wide lifecycle
/// snapshot. Persistence is prepared and installed before the in-memory
/// authority changes, so a filesystem failure cannot leave the live model and
/// restore state describing different topologies.
pub(crate) fn commit_lifecycle_snapshot_for_control(
    app: &AppHandle,
    state: &SessionState,
    candidate: &AppSessionSnapshot,
    record_derived_events: bool,
) -> Result<AppSessionSnapshot, String> {
    let refresh_window_state = control_snapshot_should_refresh_window_state(app)?;
    commit_lifecycle_snapshot_for_control_inner(
        app,
        state,
        None,
        candidate,
        record_derived_events,
        refresh_window_state,
    )
}

pub(crate) fn commit_lifecycle_snapshot_for_control_if_current(
    app: &AppHandle,
    state: &SessionState,
    expected: &AppSessionSnapshot,
    candidate: &AppSessionSnapshot,
    record_derived_events: bool,
) -> Result<AppSessionSnapshot, String> {
    let refresh_window_state = control_snapshot_should_refresh_window_state(app)?;
    commit_lifecycle_snapshot_for_control_inner(
        app,
        state,
        Some(expected),
        candidate,
        record_derived_events,
        refresh_window_state,
    )
}

/// Native window queries from the control worker can deadlock inside WebView2
/// while a child WebView is attached. Resize/focus listeners still publish
/// window state, so control commits suppress only this redundant refresh.
fn control_snapshot_should_refresh_window_state(app: &AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<crate::browser::BrowserWebviewState>() else {
        return Ok(true);
    };
    crate::browser::browser_has_any_webview_for_control(state.inner()).map(|has| !has)
}

pub(crate) fn ensure_lifecycle_snapshot_current(
    current: &AppSessionSnapshot,
    expected: &AppSessionSnapshot,
) -> Result<(), String> {
    (current == expected)
        .then_some(())
        .ok_or_else(|| "Stale lifecycle transition".to_string())
}

fn commit_lifecycle_snapshot_for_control_inner(
    app: &AppHandle,
    state: &SessionState,
    expected: Option<&AppSessionSnapshot>,
    candidate: &AppSessionSnapshot,
    record_derived_events: bool,
    refresh_window_state: bool,
) -> Result<AppSessionSnapshot, String> {
    cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(candidate)
        .and_then(|model| model.validate_indexes())
        .map_err(|error| error.to_string())?;

    let mut operations = ProductionSnapshotPublicationOperations::new(
        app,
        state,
        if record_derived_events {
            DerivedEventPolicy::Record
        } else {
            DerivedEventPolicy::Suppress
        },
    );
    operations.refresh_window_state = refresh_window_state;
    publish_snapshot_transaction(&state.snapshot, expected, candidate, &mut operations)
}
