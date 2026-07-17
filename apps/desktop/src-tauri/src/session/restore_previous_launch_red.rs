//! Durable, injectable previous-launch restore transaction.

use super::*;

struct RecordingPublication<'a> {
    authority: &'a GatedSnapshot,
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
    next_panel: &'a AtomicU64,
    original_next_panel: u64,
}

impl<'a> RecordingPublication<'a> {
    fn new(
        authority: &'a GatedSnapshot,
        original: &AppSessionSnapshot,
        next_panel: &'a AtomicU64,
    ) -> Self {
        Self {
            authority,
            calls: Vec::new(),
            persist_error: None,
            baseline: original.clone(),
            events: Vec::new(),
            next_panel,
            original_next_panel: next_panel.load(Ordering::Relaxed),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication<'_> {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.original_next_panel
        );
        self.persist_error.take().map_or(Ok(()), Err)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        assert_eq!(*self.authority.lock().unwrap(), *candidate);
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.original_next_panel
        );
        self.calls.push("authority");
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        assert_eq!(*self.authority.lock().unwrap(), *candidate);
        assert_eq!(self.baseline, *candidate);
        assert_eq!(
            self.next_panel.load(Ordering::Relaxed),
            self.original_next_panel
        );
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn restore_transaction_red(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    load_previous: impl FnOnce() -> Option<AppSessionSnapshot>,
) -> Result<AppSessionSnapshot, String> {
    let (reseed, snapshot) =
        transact_value_if_changed_snapshot(authority, publication, |candidate| {
            let Some(mut restored) = load_previous() else {
                return Ok::<_, std::convert::Infallible>((None, false));
            };
            ensure_workspace_ids(&mut restored);
            ensure_pane_ids(&mut restored);
            let reseed = next_panel_counter(&restored);
            *candidate = restored;
            Ok((Some(reseed), true))
        })
        .map_err(|error| match error {
            PaneTopologyControlError::Publication(error) => error,
            PaneTopologyControlError::Operation(error) => match error {},
        })?;
    if let Some(reseed) = reseed {
        next_panel.store(reseed, Ordering::Relaxed);
    }
    Ok(snapshot)
}

fn restored_without_stable_ids() -> AppSessionSnapshot {
    let mut restored = initial_snapshot("surface-9");
    restored.windows[0].selected_workspace_id = None;
    let workspace = &mut restored.windows[0].tab_manager.workspaces[0];
    workspace.workspace_id = None;
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() else {
        panic!("pane fixture")
    };
    pane.pane_id = None;
    restored
}

#[test]
fn missing_and_corrupt_previous_are_exact_current_noops() {
    for unavailable in ["missing", "corrupt"] {
        let current = initial_snapshot("surface-4");
        let authority = GatedSnapshot::new(current.clone());
        let next_panel = AtomicU64::new(40);
        let mut publication = RecordingPublication::new(&authority, &current, &next_panel);
        let outcome =
            restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || None)
                .unwrap();
        assert!(!outcome.restored, "{unavailable}");
        assert_eq!(outcome.snapshot, current, "{unavailable}");
        assert_eq!(*authority.lock().unwrap(), current, "{unavailable}");
        assert!(publication.calls.is_empty(), "{unavailable}");
        assert_eq!(publication.baseline, current, "{unavailable}");
        assert!(publication.events.is_empty(), "{unavailable}");
        assert_eq!(next_panel.load(Ordering::Relaxed), 40, "{unavailable}");
    }
}

#[test]
fn valid_restore_ensures_ids_and_orders_persist_authority_baseline_emit_before_reseed() {
    let current = initial_snapshot("surface-1");
    let restored = restored_without_stable_ids();
    let authority = GatedSnapshot::new(current.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&authority, &current, &next_panel);

    let committed =
        restore_transaction_red(&authority, &next_panel, &mut publication, || Some(restored))
            .unwrap();

    assert_eq!(
        publication.calls,
        ["persist", "authority", "baseline", "emit"]
    );
    assert_eq!(publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
    assert_eq!(*authority.lock().unwrap(), committed);
    assert_eq!(next_panel.load(Ordering::Relaxed), 10);
    let workspace = &committed.windows[0].tab_manager.workspaces[0];
    assert!(workspace.workspace_id.is_some());
    assert_eq!(
        committed.windows[0].selected_workspace_id,
        workspace.workspace_id
    );
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_ref() else {
        panic!("restored pane")
    };
    assert!(pane.pane_id.is_some());
}

#[test]
fn persistence_failure_preserves_every_authority_and_counter() {
    let current = initial_snapshot("surface-1");
    let restored = restored_without_stable_ids();
    let authority = GatedSnapshot::new(current.clone());
    let next_panel = AtomicU64::new(27);
    let mut publication = RecordingPublication::new(&authority, &current, &next_panel);
    publication.persist_error = Some("injected restore persistence failure".into());

    assert_eq!(
        restore_transaction_red(&authority, &next_panel, &mut publication, || Some(restored)),
        Err("injected restore persistence failure".into())
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), current);
    assert_eq!(publication.baseline, current);
    assert!(publication.events.is_empty());
    assert_eq!(next_panel.load(Ordering::Relaxed), 27);
}

#[test]
fn manual_restore_never_invokes_snapshot_persistence() {
    let current = initial_snapshot("surface-1");
    let previous = restored_without_stable_ids();
    let authority = GatedSnapshot::new(current.clone());
    let next_panel = AtomicU64::new(2);
    let mut publication = RecordingPublication::new(&authority, &current, &next_panel);

    let outcome =
        restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || {
            Some(previous)
        })
        .expect("valid previous snapshot restores");

    assert!(outcome.restored);
    assert_eq!(publication.calls, ["authority", "baseline", "emit"]);
}

struct BlockingEmitPublication {
    emitted: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

impl SnapshotPublicationOperations for BlockingEmitPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        Ok(())
    }

    fn update_event_baseline(&mut self, _candidate: &AppSessionSnapshot) {}

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.emitted.send(()).unwrap();
        self.resume.recv().unwrap();
        Ok(())
    }
}

#[test]
fn restored_authority_and_counter_are_atomic_to_concurrent_allocators() {
    let current = initial_snapshot("surface-1");
    let live_window = current.windows[0].clone();
    let restored = restored_without_stable_ids();
    let authority = GatedSnapshot::new(current);
    let next_panel = AtomicU64::new(2);
    let (emitted_tx, emitted_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();
    let (allocated_tx, allocated_rx) = std::sync::mpsc::channel();

    std::thread::scope(|scope| {
        let restore = scope.spawn(|| {
            let mut publication = BlockingEmitPublication {
                emitted: emitted_tx,
                resume: resume_rx,
            };
            restore_previous_launch_transaction(&authority, &next_panel, &mut publication, || {
                Some(restored)
            })
            .unwrap()
        });
        emitted_rx.recv().unwrap();

        let allocator = scope.spawn(|| {
            attempted_tx.send(()).unwrap();
            let _gate = authority.lock_gate();
            let allocated = next_panel.fetch_add(1, Ordering::Relaxed);
            allocated_tx.send(allocated).unwrap();
        });
        attempted_rx.recv().unwrap();
        assert_eq!(
            allocated_rx.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        );

        resume_tx.send(()).unwrap();
        let outcome = restore.join().unwrap();
        assert!(outcome.restored);
        let committed = outcome.snapshot;
        allocator.join().unwrap();
        // Additive manual restore keeps the live window, appends the restored
        // surface-9 window, and publishes the counter floor before another
        // allocator can enter the shared mutation gate.
        assert_eq!(allocated_rx.recv().unwrap(), 10);
        assert_eq!(next_panel.load(Ordering::Relaxed), 11);
        assert_eq!(committed.windows.len(), 2);
        assert_eq!(committed.windows[0], live_window);
        assert_eq!(*authority.lock().unwrap(), committed);
    });
}
