//! Durable change-gated publication for all sidebar metadata writers.

use super::*;

const NOW: i64 = 42;
const PRIORITY: Option<i64> = Some(7);

#[derive(Debug, Clone, Copy)]
enum WriterKind {
    SetProgress,
    ClearProgress,
    SetStatus,
    ClearStatus,
    SetMetadata,
    ClearMetadata,
    SetBlock,
    ClearBlock,
    Reset,
    AppendLog,
    ClearLog,
}

impl WriterKind {
    fn name(self) -> &'static str {
        match self {
            Self::SetProgress => "set_workspace_sidebar_progress_for_control",
            Self::ClearProgress => "clear_workspace_sidebar_progress_for_control",
            Self::SetStatus => "set_workspace_sidebar_status_for_control",
            Self::ClearStatus => "clear_workspace_sidebar_status_for_control",
            Self::SetMetadata => "set_workspace_sidebar_metadata_for_control",
            Self::ClearMetadata => "clear_workspace_sidebar_metadata_for_control",
            Self::SetBlock => "set_workspace_sidebar_metadata_block_for_control",
            Self::ClearBlock => "clear_workspace_sidebar_metadata_block_for_control",
            Self::Reset => "reset_workspace_sidebar_metadata_for_control",
            Self::AppendLog => "append_workspace_sidebar_log_for_control",
            Self::ClearLog => "clear_workspace_sidebar_log_for_control",
        }
    }
}

const WRITERS: [WriterKind; 11] = [
    WriterKind::SetProgress,
    WriterKind::ClearProgress,
    WriterKind::SetStatus,
    WriterKind::ClearStatus,
    WriterKind::SetMetadata,
    WriterKind::ClearMetadata,
    WriterKind::SetBlock,
    WriterKind::ClearBlock,
    WriterKind::Reset,
    WriterKind::AppendLog,
    WriterKind::ClearLog,
];

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

fn initial() -> AppSessionSnapshot {
    initial_snapshot("surface-1")
}

fn set_status(snapshot: &mut AppSessionSnapshot, value: &str) -> bool {
    let status = apply_set_workspace_sidebar_status(snapshot, 0, "build", value, PRIORITY, NOW);
    let metadata = apply_set_workspace_sidebar_metadata(
        snapshot, 0, "build", value, None, None, None, PRIORITY, None, NOW,
    );
    status || metadata
}

fn clear_status(snapshot: &mut AppSessionSnapshot, key: &str) -> bool {
    let status = apply_clear_workspace_sidebar_status(snapshot, 0, key);
    let metadata = apply_clear_workspace_sidebar_metadata(snapshot, 0, key);
    status || metadata
}

fn set_metadata(snapshot: &mut AppSessionSnapshot, value: &str) -> bool {
    let status = apply_set_workspace_sidebar_status(snapshot, 0, "review", value, PRIORITY, NOW);
    let metadata = apply_set_workspace_sidebar_metadata(
        snapshot,
        0,
        "review",
        value,
        Some("check"),
        Some("green"),
        Some("https://example.test"),
        PRIORITY,
        Some("markdown"),
        NOW,
    );
    status || metadata
}

fn clear_metadata(snapshot: &mut AppSessionSnapshot, key: &str) -> bool {
    let status = apply_clear_workspace_sidebar_status(snapshot, 0, key);
    let metadata = apply_clear_workspace_sidebar_metadata(snapshot, 0, key);
    status || metadata
}

fn seed_for_changed(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = initial();
    match kind {
        WriterKind::ClearProgress => {
            assert!(apply_set_workspace_sidebar_progress(
                &mut snapshot,
                0,
                0.5,
                Some("half")
            ));
        }
        WriterKind::ClearStatus => assert!(set_status(&mut snapshot, "running")),
        WriterKind::ClearMetadata => assert!(set_metadata(&mut snapshot, "ready")),
        WriterKind::ClearBlock => assert!(apply_set_workspace_sidebar_metadata_block(
            &mut snapshot,
            0,
            "notes",
            "hello",
            PRIORITY,
            NOW,
        )),
        WriterKind::Reset => {
            assert!(apply_set_workspace_sidebar_progress(
                &mut snapshot,
                0,
                0.5,
                Some("half")
            ));
            assert!(apply_append_workspace_sidebar_log(
                &mut snapshot,
                0,
                "seed",
                "info",
                NOW,
            ));
        }
        WriterKind::ClearLog => assert!(apply_append_workspace_sidebar_log(
            &mut snapshot,
            0,
            "seed",
            "info",
            NOW,
        )),
        _ => {}
    }
    snapshot
}

fn apply_changed(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::SetProgress => {
            apply_set_workspace_sidebar_progress(snapshot, 0, 0.5, Some("half"))
        }
        WriterKind::ClearProgress => apply_clear_workspace_sidebar_progress(snapshot, 0),
        WriterKind::SetStatus => set_status(snapshot, "running"),
        WriterKind::ClearStatus => clear_status(snapshot, "build"),
        WriterKind::SetMetadata => set_metadata(snapshot, "ready"),
        WriterKind::ClearMetadata => clear_metadata(snapshot, "review"),
        WriterKind::SetBlock => {
            apply_set_workspace_sidebar_metadata_block(snapshot, 0, "notes", "hello", PRIORITY, NOW)
        }
        WriterKind::ClearBlock => {
            apply_clear_workspace_sidebar_metadata_block(snapshot, 0, "notes")
        }
        WriterKind::Reset => apply_reset_workspace_sidebar_metadata(snapshot, 0),
        WriterKind::AppendLog => {
            apply_append_workspace_sidebar_log(snapshot, 0, "built", "info", NOW)
        }
        WriterKind::ClearLog => apply_clear_workspace_sidebar_log(snapshot, 0),
    }
}

fn seed_for_noop(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = initial();
    match kind {
        WriterKind::SetProgress => {
            assert!(apply_set_workspace_sidebar_progress(
                &mut snapshot,
                0,
                0.5,
                Some("half")
            ));
        }
        WriterKind::SetStatus | WriterKind::ClearStatus => {
            assert!(set_status(&mut snapshot, "running"));
        }
        WriterKind::SetMetadata | WriterKind::ClearMetadata => {
            assert!(set_metadata(&mut snapshot, "ready"));
        }
        WriterKind::SetBlock | WriterKind::ClearBlock => {
            assert!(apply_set_workspace_sidebar_metadata_block(
                &mut snapshot,
                0,
                "notes",
                "hello",
                PRIORITY,
                NOW,
            ));
        }
        _ => {}
    }
    snapshot
}

fn apply_noop(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::SetProgress => {
            apply_set_workspace_sidebar_progress(snapshot, 0, 0.5, Some("half"))
        }
        WriterKind::ClearProgress => apply_clear_workspace_sidebar_progress(snapshot, 99),
        WriterKind::SetStatus => set_status(snapshot, "running"),
        WriterKind::ClearStatus => clear_status(snapshot, "  "),
        WriterKind::SetMetadata => set_metadata(snapshot, "ready"),
        WriterKind::ClearMetadata => clear_metadata(snapshot, "missing"),
        WriterKind::SetBlock => {
            apply_set_workspace_sidebar_metadata_block(snapshot, 0, "notes", "hello", PRIORITY, NOW)
        }
        WriterKind::ClearBlock => apply_clear_workspace_sidebar_metadata_block(snapshot, 0, "  "),
        WriterKind::Reset => apply_reset_workspace_sidebar_metadata(snapshot, 0),
        WriterKind::AppendLog => apply_append_workspace_sidebar_log(snapshot, 0, "  ", "info", NOW),
        WriterKind::ClearLog => apply_clear_workspace_sidebar_log(snapshot, 0),
    }
}

#[test]
fn all_eleven_changed_writers_persist_then_update_baseline_then_emit_exact_snapshot() {
    for kind in WRITERS {
        let before = seed_for_changed(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(kind, candidate)
        })
        .unwrap_or_else(|error| panic!("{}: {error}", kind.name()));
        assert_ne!(committed, before, "{}", kind.name());
        assert_eq!(
            publication.calls,
            ["persist", "baseline", "emit"],
            "{}",
            kind.name()
        );
        assert_eq!(publication.baseline, committed, "{}", kind.name());
        assert_eq!(publication.events, [committed], "{}", kind.name());
    }
}

#[test]
fn all_eleven_persistence_failures_only_persist_and_leak_nothing() {
    for kind in WRITERS {
        let before = seed_for_changed(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected sidebar persistence failure".into());
        let error = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(kind, candidate)
        })
        .unwrap_err();
        assert_eq!(
            error,
            "injected sidebar persistence failure",
            "{}",
            kind.name()
        );
        assert_eq!(publication.calls, ["persist"], "{}", kind.name());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.name());
        assert_eq!(publication.baseline, before, "{}", kind.name());
        assert!(publication.events.is_empty(), "{}", kind.name());
    }
}

#[test]
fn all_eleven_same_missing_or_blank_noops_are_exact_and_perform_zero_operations() {
    for kind in WRITERS {
        let before = seed_for_noop(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_noop(kind, candidate)
        })
        .unwrap_or_else(|error| panic!("{}: {error}", kind.name()));
        assert_eq!(returned, before, "{}", kind.name());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.name());
        assert!(publication.calls.is_empty(), "{}", kind.name());
        assert_eq!(publication.baseline, before, "{}", kind.name());
        assert!(publication.events.is_empty(), "{}", kind.name());
    }
}
