//! Durable publication for pane resize, divider equalization, and split zoom.

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

fn split(divider: f64) -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    assert!(apply_split(
        &mut snapshot,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false,
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Split(root) = workspace.layout.as_mut().unwrap() else {
        panic!("split")
    };
    root.split_id = Some("split-root".into());
    root.divider_position = divider;
    let SessionWorkspaceLayoutSnapshot::Pane(first) = root.first.as_mut() else {
        panic!("first")
    };
    first.pane_id = Some("pane-a".into());
    let SessionWorkspaceLayoutSnapshot::Pane(second) = root.second.as_mut() else {
        panic!("second")
    };
    second.pane_id = Some("pane-b".into());
    snapshot
}

fn divider(snapshot: &AppSessionSnapshot) -> f64 {
    let SessionWorkspaceLayoutSnapshot::Split(root) = snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    else {
        panic!("split")
    };
    root.divider_position
}

fn resize_candidate(
    snapshot: &mut AppSessionSnapshot,
    pane_id: &str,
    intent: PaneResizeControlIntent,
) -> Result<session_ops::PaneResizeResult, PaneResizeControlError> {
    let workspace = snapshot
        .windows
        .get_mut(0)
        .and_then(|window| window.tab_manager.workspaces.get_mut(0))
        .ok_or(PaneResizeControlError::WorkspaceNotFound)?;
    match intent {
        PaneResizeControlIntent::Relative { direction, amount } => {
            session_ops::resize_pane_relative(workspace, pane_id, direction, amount, 100.0, 100.0)
        }
        PaneResizeControlIntent::Absolute {
            axis,
            target_pixels,
        } => {
            session_ops::resize_pane_absolute(workspace, pane_id, axis, target_pixels, 100.0, 100.0)
        }
    }
    .map_err(PaneResizeControlError::Pane)
}

fn assert_publication(publication: &RecordingPublication, committed: &AppSessionSnapshot) {
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(&publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
}

#[test]
fn resize_success_always_publishes_changed_exact_divider_and_zero_amount_values() {
    for (intent, expected) in [
        (
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Right,
                amount: 10,
            },
            0.6,
        ),
        (
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Right,
                amount: 0,
            },
            0.5,
        ),
        (
            PaneResizeControlIntent::Absolute {
                axis: SessionSplitOrientation::Horizontal,
                target_pixels: 50.0,
            },
            0.5,
        ),
    ] {
        let before = split(0.5);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let (resized, committed) =
            transact_pane_topology_snapshot(&authority, &mut publication, |candidate| {
                resize_candidate(candidate, "pane-a", intent)
            })
            .unwrap();
        assert_eq!(resized.split_id, "split-root");
        assert_eq!(resized.old_divider_position, 0.5);
        assert_eq!(resized.new_divider_position, expected);
        assert_eq!(divider(&committed), expected);
        assert_publication(&publication, &committed);
    }
}

#[test]
fn every_resize_domain_error_is_atomic_with_zero_publication_operations() {
    let mut missing_workspace = split(0.5);
    missing_workspace.windows.clear();
    let mut missing_identity = split(0.5);
    let SessionWorkspaceLayoutSnapshot::Split(root) =
        missing_identity.windows[0].tab_manager.workspaces[0]
            .layout
            .as_mut()
            .unwrap()
    else {
        panic!("split")
    };
    root.split_id = None;
    let cases = [
        (
            missing_workspace,
            "pane-a",
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Right,
                amount: 10,
            },
            "workspace",
        ),
        (
            split(0.5),
            "missing",
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Right,
                amount: 10,
            },
            "pane",
        ),
        (
            split(0.5),
            "pane-a",
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Up,
                amount: 10,
            },
            "orientation",
        ),
        (
            split(0.5),
            "pane-a",
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Left,
                amount: 10,
            },
            "border",
        ),
        (
            missing_identity,
            "pane-a",
            PaneResizeControlIntent::Relative {
                direction: session_ops::PaneResizeDirection::Right,
                amount: 10,
            },
            "identity",
        ),
    ];
    for (before, pane_id, intent, expected) in cases {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let error = transact_pane_topology_snapshot(&authority, &mut publication, |candidate| {
            resize_candidate(candidate, pane_id, intent)
        })
        .unwrap_err();
        match (expected, error) {
            (
                "workspace",
                PaneTopologyControlError::Operation(PaneResizeControlError::WorkspaceNotFound),
            )
            | (
                "pane",
                PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
                    session_ops::PaneResizeError::PaneNotFoundInTree,
                )),
            )
            | (
                "orientation",
                PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
                    session_ops::PaneResizeError::NoOrientationSplitAncestor,
                )),
            )
            | (
                "border",
                PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
                    session_ops::PaneResizeError::NoAdjacentBorder,
                )),
            )
            | (
                "identity",
                PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
                    session_ops::PaneResizeError::MissingSplitIdentity,
                )),
            ) => {}
            (_, error) => panic!("unexpected {expected} error: {error:?}"),
        }
        assert_eq!(*authority.lock().unwrap(), before);
        assert!(publication.calls.is_empty());
        assert!(publication.events.is_empty());
    }
}

#[test]
fn equalize_always_publishes_for_changed_and_applicator_false_layouts() {
    let before = split(0.3);
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed = transact_snapshot_always(&authority, &mut publication, |candidate| {
        apply_equalize_dividers(candidate)
    })
    .unwrap();
    assert_eq!(divider(&committed), 0.5);
    assert_publication(&publication, &committed);

    let before = initial_snapshot("only");
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed = transact_snapshot_always(&authority, &mut publication, |candidate| {
        apply_equalize_dividers(candidate)
    })
    .unwrap();
    assert_eq!(committed, before);
    assert_publication(&publication, &committed);
}

#[test]
fn zoom_set_and_clear_publish_while_missing_and_single_pane_are_exact_noops() {
    let authority = GatedSnapshot::new(split(0.5));
    for expected in [Some("a"), None] {
        let initial = authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_toggle_split_zoom(candidate, "a")
        })
        .unwrap();
        assert_eq!(
            committed.windows[0].tab_manager.workspaces[0]
                .zoomed_panel_id
                .as_deref(),
            expected
        );
        assert_publication(&publication, &committed);
    }

    for (before, panel_id) in [(split(0.5), "missing"), (initial_snapshot("only"), "only")] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_toggle_split_zoom(candidate, panel_id)
        })
        .unwrap();
        assert_eq!(returned, before);
        assert!(publication.calls.is_empty());
        assert!(publication.events.is_empty());
    }
}

#[test]
fn persistence_failures_only_persist_for_resize_equalize_and_zoom() {
    let before = split(0.5);
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("resize persistence failure".into());
    assert!(matches!(
        transact_pane_topology_snapshot(&authority, &mut publication, |candidate| {
            resize_candidate(
                candidate,
                "pane-a",
                PaneResizeControlIntent::Relative {
                    direction: session_ops::PaneResizeDirection::Right,
                    amount: 10,
                },
            )
        }),
        Err(PaneTopologyControlError::Publication(error)) if error == "resize persistence failure"
    ));
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());

    for (message, mutation) in [
        (
            "equalize persistence failure",
            apply_equalize_dividers as fn(&mut AppSessionSnapshot) -> bool,
        ),
        (
            "zoom persistence failure",
            (|candidate: &mut AppSessionSnapshot| apply_toggle_split_zoom(candidate, "a"))
                as fn(&mut AppSessionSnapshot) -> bool,
        ),
    ] {
        let before = split(0.3);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some(message.into());
        let error = if message.starts_with("equalize") {
            transact_snapshot_always(&authority, &mut publication, mutation).unwrap_err()
        } else {
            transact_snapshot_if_changed(&authority, &mut publication, mutation).unwrap_err()
        };
        assert_eq!(error, message);
        assert_eq!(publication.calls, ["persist"]);
        assert_eq!(*authority.lock().unwrap(), before);
        assert_eq!(publication.baseline, before);
        assert!(publication.events.is_empty());
    }
}

#[test]
fn concurrent_resize_and_zoom_writers_serialize_before_mutation() {
    let authority = Arc::new(GatedSnapshot::new(split(0.5)));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (second_tx, second_rx) = mpsc::channel();
    let first_authority = Arc::clone(&authority);
    let first = std::thread::spawn(move || {
        let initial = first_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_pane_topology_snapshot(&first_authority, &mut publication, |candidate| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            resize_candidate(
                candidate,
                "pane-a",
                PaneResizeControlIntent::Relative {
                    direction: session_ops::PaneResizeDirection::Right,
                    amount: 10,
                },
            )
        })
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let second_authority = Arc::clone(&authority);
    let second = std::thread::spawn(move || {
        let initial = second_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_snapshot_if_changed(&second_authority, &mut publication, |candidate| {
            second_tx.send(()).unwrap();
            apply_toggle_split_zoom(candidate, "a")
        })
        .unwrap();
    });
    assert!(second_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    let committed = authority.lock().unwrap();
    assert_eq!(divider(&committed), 0.6);
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[0]
            .zoomed_panel_id
            .as_deref(),
        Some("a")
    );
}
