//! Durable transaction contract for terminal-only structural writers.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(initial: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: initial.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        match self.persist_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum StructuralKind {
    Split,
    Tab,
}

fn initial() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].workspace_environment =
        Some(BTreeMap::from([("INHERITED".into(), "yes".into())]));
    snapshot
}

fn apply_structural(
    snapshot: &mut AppSessionSnapshot,
    kind: StructuralKind,
    target: &str,
    new_panel_id: &str,
) -> bool {
    let environment = Some(BTreeMap::from([("REQUESTED".into(), "yes".into())]));
    match kind {
        StructuralKind::Split => apply_split_with_terminal_startup(
            snapshot,
            target,
            SessionSplitOrientation::Horizontal,
            new_panel_id,
            false,
            Some("cargo test"),
            Some("echo ready"),
            environment,
        ),
        StructuralKind::Tab => apply_new_terminal_tab(
            snapshot,
            target,
            new_panel_id,
            Some("cargo test"),
            Some("echo ready"),
            environment,
        ),
    }
}

fn run_structural(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    operations: &mut impl SnapshotPublicationOperations,
    kind: StructuralKind,
    target: &str,
) -> Result<(String, AppSessionSnapshot), String> {
    let _outer_gate = authority.lock_gate();
    let current = authority.lock().unwrap().clone();
    let mut validation = current.clone();
    if !apply_structural(&mut validation, kind, target, "surface-validation") {
        return Err(format!("no pane holds panel id {target}"));
    }
    let new_panel_id = format!("surface-{}", next_panel.fetch_add(1, Ordering::Relaxed));
    transact_lifecycle_snapshot(authority, operations, |candidate| {
        assert!(apply_structural(candidate, kind, target, &new_panel_id));
        Ok(new_panel_id.clone())
    })
}

fn startup<'a>(
    snapshot: &'a AppSessionSnapshot,
    panel_id: &str,
) -> &'a SessionPanelTerminalStartupSnapshot {
    snapshot.windows[0].tab_manager.workspaces[0]
        .panel_terminal_startups
        .as_ref()
        .unwrap()
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .unwrap()
}

#[test]
fn valid_split_and_tab_persist_then_baseline_then_emit_exact_startup_snapshot() {
    for kind in [StructuralKind::Split, StructuralKind::Tab] {
        let before = initial();
        let authority = GatedSnapshot::new(before.clone());
        let next_panel = AtomicU64::new(2);
        let mut publication = RecordingPublication::new(&before);

        let (panel_id, committed) =
            run_structural(&authority, &next_panel, &mut publication, kind, "surface-1").unwrap();

        assert_eq!(panel_id, "surface-2");
        assert_eq!(next_panel.load(Ordering::Relaxed), 3);
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
        assert_eq!(*authority.lock().unwrap(), committed);
        assert_eq!(publication.baseline, committed);
        assert_eq!(publication.events, [committed.clone()]);
        let startup = startup(&committed, &panel_id);
        assert_eq!(
            startup.initial_terminal_command.as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            startup.initial_terminal_input.as_deref(),
            Some("echo ready")
        );
        assert_eq!(
            startup.initial_terminal_environment.as_ref().unwrap(),
            &BTreeMap::from([
                ("INHERITED".to_string(), "yes".to_string()),
                ("REQUESTED".to_string(), "yes".to_string()),
            ])
        );
    }
}

#[test]
fn invalid_target_is_exact_zero_operation_and_does_not_reserve_an_identity() {
    for kind in [StructuralKind::Split, StructuralKind::Tab] {
        let before = initial();
        let authority = GatedSnapshot::new(before.clone());
        let next_panel = AtomicU64::new(2);
        let mut publication = RecordingPublication::new(&before);

        assert_eq!(
            run_structural(&authority, &next_panel, &mut publication, kind, "missing").unwrap_err(),
            "no pane holds panel id missing"
        );
        assert_eq!(next_panel.load(Ordering::Relaxed), 2);
        assert_eq!(*authority.lock().unwrap(), before);
        assert_eq!(publication.baseline, before);
        assert!(publication.calls.is_empty());
        assert!(publication.events.is_empty());
    }
}

#[test]
fn persistence_failure_has_no_orphan_panel_in_authority_baseline_or_events() {
    for kind in [StructuralKind::Split, StructuralKind::Tab] {
        let before = initial();
        let authority = GatedSnapshot::new(before.clone());
        let next_panel = AtomicU64::new(2);
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected structural persistence failure".into());

        assert_eq!(
            run_structural(&authority, &next_panel, &mut publication, kind, "surface-1")
                .unwrap_err(),
            "injected structural persistence failure"
        );
        assert_eq!(
            next_panel.load(Ordering::Relaxed),
            3,
            "counter gaps are allowed"
        );
        assert_eq!(publication.calls, ["persist"]);
        assert_eq!(*authority.lock().unwrap(), before);
        assert_eq!(publication.baseline, before);
        assert!(publication.events.is_empty());
    }
}

#[test]
fn concurrent_structural_writers_serialize_before_validation_and_reservation() {
    let before = initial();
    let authority = Arc::new(GatedSnapshot::new(before));
    let next_panel = Arc::new(AtomicU64::new(2));
    let (first_reserved_tx, first_reserved_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let (second_entered_tx, second_entered_rx) = mpsc::channel();

    let first_authority = Arc::clone(&authority);
    let first_counter = Arc::clone(&next_panel);
    let first = std::thread::spawn(move || {
        let mut publication = RecordingPublication::new(&first_authority.lock().unwrap());
        let _outer = first_authority.lock_gate();
        let current = first_authority.lock().unwrap().clone();
        let new_id = format!("surface-{}", first_counter.fetch_add(1, Ordering::Relaxed));
        first_reserved_tx.send(()).unwrap();
        release_first_rx.recv().unwrap();
        transact_lifecycle_snapshot(&first_authority, &mut publication, |candidate| {
            assert!(apply_structural(
                candidate,
                StructuralKind::Tab,
                "surface-1",
                &new_id
            ));
            Ok(())
        })
        .unwrap();
        current
    });
    first_reserved_rx.recv().unwrap();

    let second_authority = Arc::clone(&authority);
    let second_counter = Arc::clone(&next_panel);
    let second = std::thread::spawn(move || {
        second_entered_tx.send(()).unwrap();
        let mut publication = RecordingPublication::new(&second_authority.lock().unwrap());
        run_structural(
            &second_authority,
            &second_counter,
            &mut publication,
            StructuralKind::Tab,
            "surface-1",
        )
        .unwrap();
    });
    second_entered_rx.recv().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(next_panel.load(Ordering::Relaxed), 3);
    release_first_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(next_panel.load(Ordering::Relaxed), 4);
}
