//! Durable publication contract for the three pure always-publish commands.

use super::*;

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

    fn failing(initial: &AppSessionSnapshot) -> Self {
        Self {
            persist_error: Some("injected always-publish persistence failure".into()),
            ..Self::new(initial)
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

type Mutation = fn(&mut AppSessionSnapshot) -> bool;

struct WriterCase {
    command: &'static str,
    changed_initial: fn() -> AppSessionSnapshot,
    change: Mutation,
    no_op_initial: fn() -> AppSessionSnapshot,
    no_op: Mutation,
}

fn initial() -> AppSessionSnapshot {
    initial_snapshot("surface-1")
}

fn split_initial() -> AppSessionSnapshot {
    let mut snapshot = initial();
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    snapshot
}

fn skewed_split_initial() -> AppSessionSnapshot {
    let mut snapshot = split_initial();
    assert!(apply_set_divider(&mut snapshot, &[], 0.85));
    snapshot
}

fn change_divider(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_divider(snapshot, &[], 0.25)
}

fn noop_divider(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_divider(snapshot, &[], 0.25)
}

fn change_equalize(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_equalize_dividers(snapshot)
}

fn noop_equalize(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_equalize_dividers(snapshot)
}

fn change_surface_kind(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_surface_kind(snapshot, "surface-1", Some("agent".into()))
}

fn noop_surface_kind(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_surface_kind(snapshot, "missing", Some("agent".into()))
}

fn cases() -> [WriterCase; 3] {
    [
        WriterCase {
            command: "session_set_divider",
            changed_initial: split_initial,
            change: change_divider,
            no_op_initial: initial,
            no_op: noop_divider,
        },
        WriterCase {
            command: "session_equalize_dividers",
            changed_initial: skewed_split_initial,
            change: change_equalize,
            no_op_initial: initial,
            no_op: noop_equalize,
        },
        WriterCase {
            command: "session_set_surface_kind",
            changed_initial: initial,
            change: change_surface_kind,
            no_op_initial: initial,
            no_op: noop_surface_kind,
        },
    ]
}

fn assert_successful_publication(
    case: &WriterCase,
    before: AppSessionSnapshot,
    mutation: Mutation,
    expect_change: bool,
) {
    let mut expected = before.clone();
    assert_eq!(mutation(&mut expected), expect_change, "{}", case.command);
    assert_eq!(expected != before, expect_change, "{}", case.command);
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);

    let result: Result<AppSessionSnapshot, String> =
        transact_snapshot_always(&authority, &mut publication, mutation);
    let committed = result.unwrap_or_else(|error| panic!("{}: {error}", case.command));

    assert_eq!(committed != before, expect_change, "{}", case.command);
    assert_eq!(
        publication.calls,
        ["persist", "baseline", "emit"],
        "{}",
        case.command
    );
    assert_eq!(*authority.lock().unwrap(), committed, "{}", case.command);
    assert_eq!(publication.baseline, committed, "{}", case.command);
    assert_eq!(publication.events, [committed.clone()], "{}", case.command);
}

#[test]
fn changed_and_applicator_false_paths_both_persist_then_update_baseline_then_emit() {
    for case in cases() {
        assert_successful_publication(&case, (case.changed_initial)(), case.change, true);
        assert_successful_publication(&case, (case.no_op_initial)(), case.no_op, false);
    }
}

#[test]
fn persistence_failure_on_changed_and_applicator_false_paths_preserves_all_authority() {
    for case in cases() {
        for (before, mutation, expect_change) in [
            ((case.changed_initial)(), case.change, true),
            ((case.no_op_initial)(), case.no_op, false),
        ] {
            let mut witness = before.clone();
            assert_eq!(mutation(&mut witness), expect_change, "{}", case.command);
            assert_eq!(witness != before, expect_change, "{}", case.command);
            let authority = GatedSnapshot::new(before.clone());
            let mut publication = RecordingPublication::failing(&before);

            let error =
                transact_snapshot_always(&authority, &mut publication, mutation).unwrap_err();

            assert_eq!(
                error, "injected always-publish persistence failure",
                "{}",
                case.command
            );
            assert_eq!(publication.calls, ["persist"], "{}", case.command);
            assert_eq!(*authority.lock().unwrap(), before, "{}", case.command);
            assert_eq!(publication.baseline, before, "{}", case.command);
            assert!(publication.events.is_empty(), "{} emitted", case.command);
        }
    }
}

fn command_source<'a>(source: &'a str, command: &str) -> &'a str {
    let start = source
        .find(&format!("pub fn {command}("))
        .unwrap_or_else(|| panic!("missing production command {command}"));
    let tail = &source[start..];
    let body_start = tail.find('{').expect("command body");
    let mut depth = 0usize;
    let mut end = tail.len();
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + offset + character.len_utf8();
                    break;
                }
            }
            _ => {}
        }
    }
    &tail[..end]
}

#[test]
fn all_three_production_commands_route_through_the_shared_fallible_always_publish_seam() {
    let source = include_str!("../session.rs");
    for case in cases() {
        let body = command_source(source, case.command);
        let compact = body
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            compact.contains(")->Result<AppSessionSnapshot,String>{"),
            "{} did not preserve Result<AppSessionSnapshot, String>",
            case.command
        );
        assert!(
            compact.contains("state.transact_snapshot_always(&app,"),
            "{} bypasses SessionState::transact_snapshot_always",
            case.command
        );
        assert!(
            !compact.contains("state.snapshot.lock()")
                && !compact.contains("notify_session_changed("),
            "{} still owns a direct lock/notify publication path",
            case.command
        );
    }
}
