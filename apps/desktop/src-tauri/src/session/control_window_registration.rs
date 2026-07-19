use super::*;

fn transact_register_window_with(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window_id: &str,
    build_window: impl FnOnce() -> SessionWindowSnapshot,
) -> Result<RegisterWindowOutcome, String> {
    let _transaction_guard = authority.lock_gate();
    let before = transaction_current_snapshot(authority)?;
    if window_id.trim().is_empty() {
        return Ok(RegisterWindowOutcome::Unchanged(before));
    }
    if before
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == Some(window_id))
    {
        return Ok(RegisterWindowOutcome::Unchanged(before));
    }
    if window_id == "main" && !before.windows.is_empty() {
        let mut candidate = before.clone();
        candidate.windows[0].window_id = Some("main".to_string());
        let committed =
            publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
        return Ok(RegisterWindowOutcome::Registered(committed));
    }
    let mut candidate = before.clone();
    candidate.windows.push(build_window());
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(RegisterWindowOutcome::Registered(committed))
}

pub(super) fn transact_register_window(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window_id: &str,
) -> Result<RegisterWindowOutcome, String> {
    transact_register_window_with(authority, next_panel, publication, window_id, || {
        let panel_id = Uuid::new_v4().to_string();
        auxiliary_window_snapshot(window_id, &panel_id)
    })
}

pub(super) fn transact_register_prepared_window(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window: &SessionWindowSnapshot,
) -> Result<RegisterWindowOutcome, String> {
    let window_id = window
        .window_id
        .as_deref()
        .ok_or_else(|| "Prepared window is missing its identity".to_string())?;
    transact_register_window_with(authority, next_panel, publication, window_id, || {
        window.clone()
    })
}

pub(crate) fn register_prepared_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window: &SessionWindowSnapshot,
) -> Result<RegisterWindowOutcome, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    publication.derived_events = DerivedEventPolicy::Suppress;
    transact_register_prepared_window(&state.snapshot, &state.next_panel, &mut publication, window)
}

pub(crate) fn unregister_window_for_control_suppressing_events(
    app: &AppHandle,
    state: &SessionState,
    window_id: &str,
) -> Result<UnregisterWindowOutcome, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Suppress);
    transact_unregister_window(&state.snapshot, &mut publication, window_id)
}
