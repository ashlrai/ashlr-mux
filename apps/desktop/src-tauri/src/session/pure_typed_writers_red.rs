//! Fallible publication for strict and optional pure model writers.

use super::*;

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(snapshot: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: snapshot.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        self.persist_error.take().map_or(Ok(()), Err)
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

fn optional_value_transaction(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
) -> Result<Option<AppSessionSnapshot>, String> {
    let (resolved, snapshot) =
        transact_value_if_changed_snapshot(authority, publication, |candidate| {
            let resolved = mutation(candidate);
            Ok::<_, std::convert::Infallible>((resolved, resolved))
        })
        .map_err(|error| match error {
            PaneTopologyControlError::Publication(error) => error,
            PaneTopologyControlError::Operation(error) => match error {},
        })?;
    Ok(resolved.then_some(snapshot))
}

fn strict_zoom_transaction(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    panel_id: &str,
    zoom: f64,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<&'static str>> {
    let ((), snapshot) = transact_pane_topology_snapshot(authority, publication, |candidate| {
        apply_set_browser_zoom(candidate, panel_id, zoom)
            .then_some(())
            .ok_or("unable to set browser zoom")
    })?;
    Ok(snapshot)
}

#[test]
fn strict_zoom_valid_and_same_publish_while_missing_is_exact_domain_error() {
    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let changed = strict_zoom_transaction(&authority, &mut publication, "surface-1", 9.0).unwrap();
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
        changed.windows[0].tab_manager.workspaces[0].layout.as_ref()
    else {
        panic!("pane")
    };
    assert_eq!(pane.browser_page_zoom, Some(3.0));

    let authority = GatedSnapshot::new(changed.clone());
    let mut publication = RecordingPublication::new(&changed);
    let same = strict_zoom_transaction(&authority, &mut publication, "surface-1", 99.0).unwrap();
    assert_eq!(same, changed);
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);

    let authority = GatedSnapshot::new(changed.clone());
    let mut publication = RecordingPublication::new(&changed);
    assert_eq!(
        strict_zoom_transaction(&authority, &mut publication, "missing", 1.0),
        Err(PaneTopologyControlError::Operation(
            "unable to set browser zoom"
        ))
    );
    assert!(publication.calls.is_empty());
    assert_eq!(*authority.lock().unwrap(), changed);
}

#[test]
fn rename_changed_normalized_and_same_or_missing_noops_follow_change_gate() {
    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let renamed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
        apply_rename_workspace(candidate, 0, "  Review  ")
    })
    .unwrap();
    assert_eq!(
        renamed.windows[0].tab_manager.workspaces[0]
            .custom_title
            .as_deref(),
        Some("Review")
    );
    assert_eq!(
        renamed.windows[0].tab_manager.workspaces[0]
            .custom_title_source
            .as_deref(),
        Some("user")
    );
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);

    for index in [0, 99] {
        let authority = GatedSnapshot::new(renamed.clone());
        let mut publication = RecordingPublication::new(&renamed);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_rename_workspace(candidate, index, "Review")
        })
        .unwrap();
        assert_eq!(returned, renamed);
        assert!(publication.calls.is_empty());
    }
}

fn started(panel_id: &str) -> StartedAgentSessionSnapshot {
    StartedAgentSessionSnapshot {
        panel_id: panel_id.into(),
        workspace_id: None,
        provider_id: "codex".into(),
        session_id: "session-1".into(),
        executable_path: " C:\\Program Files\\Codex\\codex.exe ".into(),
        arguments: vec!["app-server".into()],
        working_directory: Some("C:\\repo".into()),
    }
}

fn apply_started(snapshot: &mut AppSessionSnapshot, started: &StartedAgentSessionSnapshot) -> bool {
    apply_restorable_agent_snapshot(
        snapshot,
        started.workspace_id.as_deref(),
        &started.panel_id,
        restorable_snapshot_from_started(started),
    )
}

#[test]
fn started_agent_returns_exact_changed_bool_noop_bool_and_rolls_back_persist_failure() {
    let started = started("surface-1");
    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (changed, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_started(candidate, &started);
            Ok::<_, std::convert::Infallible>((changed, changed))
        })
        .unwrap();
    assert!(changed);
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    let recorded = &committed.windows[0].tab_manager.workspaces[0]
        .restorable_agent_snapshots
        .as_ref()
        .unwrap()[0];
    assert_eq!(recorded.panel_id, "surface-1");
    assert!(recorded
        .snapshot
        .resume_command
        .as_ref()
        .unwrap()
        .contains("codex.exe"));

    let authority = GatedSnapshot::new(committed.clone());
    let mut publication = RecordingPublication::new(&committed);
    let (changed, same) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_started(candidate, &started);
            Ok::<_, std::convert::Infallible>((changed, changed))
        })
        .unwrap();
    assert!(!changed);
    assert_eq!(same, committed);
    assert!(publication.calls.is_empty());

    let before = initial();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected agent persistence failure".into());
    assert!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_started(candidate, &started);
            Ok::<_, std::convert::Infallible>((changed, changed))
        },)
        .is_err()
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
}

#[derive(Clone, Copy)]
enum OptionalWriter {
    Markdown,
    File,
    Sidebar,
}

impl OptionalWriter {
    fn helper(self) -> &'static str {
        match self {
            Self::Markdown => "open_markdown_file_in_panel",
            Self::File => "open_file_in_panel",
            Self::Sidebar => "open_custom_sidebar_in_panel",
        }
    }

    fn apply(self, snapshot: &mut AppSessionSnapshot, panel_id: &str, path: &str) -> bool {
        match self {
            Self::Markdown => apply_open_markdown_file(snapshot, panel_id, path),
            Self::File => apply_open_file(snapshot, panel_id, path),
            Self::Sidebar => apply_open_custom_sidebar(snapshot, panel_id, path),
        }
    }
}

const OPTIONAL_WRITERS: [OptionalWriter; 3] = [
    OptionalWriter::Markdown,
    OptionalWriter::File,
    OptionalWriter::Sidebar,
];

#[test]
fn optional_openers_publish_valid_and_repeat_but_resolve_missing_or_blank_to_none() {
    for writer in OPTIONAL_WRITERS {
        let before = initial();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let opened = optional_value_transaction(&authority, &mut publication, |candidate| {
            writer.apply(candidate, "surface-1", "  C:\\repo\\README.md  ")
        })
        .unwrap()
        .unwrap();
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);

        let authority = GatedSnapshot::new(opened.clone());
        let mut publication = RecordingPublication::new(&opened);
        let repeated = optional_value_transaction(&authority, &mut publication, |candidate| {
            writer.apply(candidate, "surface-1", "C:\\repo\\README.md")
        })
        .unwrap()
        .unwrap();
        assert_eq!(repeated, opened, "{}", writer.helper());
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);

        for (panel_id, path) in [("missing", "C:\\repo\\README.md"), ("surface-1", "  ")] {
            let authority = GatedSnapshot::new(opened.clone());
            let mut publication = RecordingPublication::new(&opened);
            assert_eq!(
                optional_value_transaction(&authority, &mut publication, |candidate| writer
                    .apply(candidate, panel_id, path))
                .unwrap(),
                None,
                "{}",
                writer.helper()
            );
            assert!(publication.calls.is_empty());
            assert_eq!(*authority.lock().unwrap(), opened);
        }
    }
}

#[test]
fn strict_rename_agent_and_optional_persist_failures_preserve_all_authority() {
    let before = initial();
    for mutation in [0usize, 1, 2, 3, 4, 5] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected typed persistence failure".into());
        let failed = match mutation {
            0 => strict_zoom_transaction(&authority, &mut publication, "surface-1", 2.0).is_err(),
            1 => transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
                apply_rename_workspace(candidate, 0, "Review")
            })
            .is_err(),
            2 => transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
                let changed = apply_started(candidate, &started("surface-1"));
                Ok::<_, std::convert::Infallible>((changed, changed))
            })
            .is_err(),
            index => optional_value_transaction(&authority, &mut publication, |candidate| {
                OPTIONAL_WRITERS[index - 3].apply(candidate, "surface-1", "C:\\repo\\README.md")
            })
            .is_err(),
        };
        assert!(failed);
        assert_eq!(publication.calls, ["persist"]);
        assert_eq!(*authority.lock().unwrap(), before);
        assert_eq!(publication.baseline, before);
        assert!(publication.events.is_empty());
    }
}
