//! Fault injection and production-routing contract for change-gated direct writers.

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
            persist_error: Some("injected direct writer persistence failure".into()),
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
    initial: fn() -> AppSessionSnapshot,
    change: Mutation,
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

fn adjacent_initial() -> AppSessionSnapshot {
    let mut snapshot = initial();
    let layout = active_layout_slot(&mut snapshot)
        .expect("layout slot")
        .as_mut()
        .expect("layout");
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = layout else {
        panic!("expected pane")
    };
    pane.panel_ids.push("surface-2".into());
    snapshot
}

fn canvas_initial() -> AppSessionSnapshot {
    let mut snapshot = split_initial();
    assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));
    snapshot
}

fn change_process_title(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_process_title(snapshot, "surface-1", "cargo test")
}

fn noop_process_title(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_process_title(snapshot, "missing", "ignored")
}

fn change_toggle_zoom(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_toggle_split_zoom(snapshot, "surface-2")
}

fn noop_toggle_zoom(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_toggle_split_zoom(snapshot, "missing")
}

fn change_layout_mode(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_layout_mode(snapshot, Some("canvas"))
}

fn noop_layout_mode(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_layout_mode(snapshot, None)
}

fn change_canvas_frame(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_canvas_pane_frame(snapshot, "surface-1", 24, 32, 640, 360)
}

fn noop_canvas_frame(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_set_canvas_pane_frame(snapshot, " ", 24, 32, 640, 360)
}

fn change_canvas_action(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_canvas_action(snapshot, "distributeVertically", None)
}

fn noop_canvas_action(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_canvas_action(snapshot, "doesNotExist", None)
}

fn change_adjacent_selection(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_select_adjacent_panel(snapshot, "surface-1", true)
}

fn noop_adjacent_selection(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_select_adjacent_panel(snapshot, "surface-1", true)
}

fn change_focus(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_focus_panel(snapshot, "surface-2")
}

fn noop_focus(snapshot: &mut AppSessionSnapshot) -> bool {
    apply_focus_panel(snapshot, "surface-1")
}

fn cases() -> [WriterCase; 7] {
    [
        WriterCase {
            command: "session_set_process_title",
            initial,
            change: change_process_title,
            no_op: noop_process_title,
        },
        WriterCase {
            command: "session_toggle_split_zoom",
            initial: split_initial,
            change: change_toggle_zoom,
            no_op: noop_toggle_zoom,
        },
        WriterCase {
            command: "session_set_layout_mode",
            initial,
            change: change_layout_mode,
            no_op: noop_layout_mode,
        },
        WriterCase {
            command: "session_set_canvas_pane_frame",
            initial: canvas_initial,
            change: change_canvas_frame,
            no_op: noop_canvas_frame,
        },
        WriterCase {
            command: "session_apply_canvas_action",
            initial: canvas_initial,
            change: change_canvas_action,
            no_op: noop_canvas_action,
        },
        WriterCase {
            command: "session_select_adjacent_panel",
            initial: adjacent_initial,
            change: change_adjacent_selection,
            no_op: noop_adjacent_selection,
        },
        WriterCase {
            command: "session_focus_panel",
            initial: split_initial,
            change: change_focus,
            no_op: noop_focus,
        },
    ]
}

#[test]
fn every_changed_direct_writer_persists_then_updates_baseline_then_emits_exact_success() {
    for case in cases() {
        let before = (case.initial)();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);

        let result: Result<AppSessionSnapshot, String> =
            transact_snapshot_if_changed(&authority, &mut publication, case.change);
        let committed = result.unwrap_or_else(|error| panic!("{}: {error}", case.command));

        assert_ne!(
            committed, before,
            "{} did not exercise a change",
            case.command
        );
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
}

#[test]
fn every_direct_writer_persist_failure_preserves_authority_baseline_and_events() {
    for case in cases() {
        let before = (case.initial)();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::failing(&before);

        let error =
            transact_snapshot_if_changed(&authority, &mut publication, case.change).unwrap_err();

        assert_eq!(
            error, "injected direct writer persistence failure",
            "{}",
            case.command
        );
        assert_eq!(publication.calls, ["persist"], "{}", case.command);
        assert_eq!(*authority.lock().unwrap(), before, "{}", case.command);
        assert_eq!(publication.baseline, before, "{}", case.command);
        assert!(publication.events.is_empty(), "{} emitted", case.command);
    }
}

#[test]
fn every_direct_writer_no_op_returns_exact_current_snapshot_with_zero_publication_operations() {
    for case in cases() {
        let before = initial();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);

        let returned = transact_snapshot_if_changed(&authority, &mut publication, case.no_op)
            .unwrap_or_else(|error| panic!("{}: {error}", case.command));

        assert_eq!(returned, before, "{}", case.command);
        assert_eq!(*authority.lock().unwrap(), before, "{}", case.command);
        assert!(publication.calls.is_empty(), "{} published", case.command);
        assert_eq!(publication.baseline, before, "{}", case.command);
        assert!(publication.events.is_empty(), "{} emitted", case.command);
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
fn all_seven_production_commands_route_through_the_shared_fallible_session_state_seam() {
    let source = include_str!("../session.rs");
    for case in cases() {
        let body = command_source(source, case.command);
        let compact = body
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            compact.contains(")->Result<AppSessionSnapshot,String>{"),
            "{} did not preserve the required Result<AppSessionSnapshot, String> success shape",
            case.command
        );
        if case.command == "session_select_adjacent_panel" {
            assert!(
                compact.contains("select_adjacent_panel_for_control(&app,&state,"),
                "{} bypasses the shared typed focus-navigation helper",
                case.command
            );
        } else if case.command == "session_toggle_split_zoom" {
            assert!(
                compact.contains("toggle_split_zoom_for_control(&app,&state,"),
                "{} bypasses the shared pane-layout helper",
                case.command
            );
        } else {
            assert!(
                compact.contains("state.transact_snapshot_if_changed(&app,"),
                "{} bypasses SessionState::transact_snapshot_if_changed",
                case.command
            );
        }
        assert!(
            !compact.contains("state.snapshot.lock()"),
            "{} still mutates live authority before persistence",
            case.command
        );
    }
}
