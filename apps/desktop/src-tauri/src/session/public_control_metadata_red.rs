//! Durable change-gated publication for paired public/control metadata writers.

use super::*;

const WORKSPACE_UNREAD_AT: i64 = 41;
const PANEL_UNREAD_AT: i64 = 43;
const GROUP_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[derive(Debug, Clone, Copy)]
enum WriterKind {
    WorkspaceDescription,
    WorkspaceResetColor,
    WorkspaceUnread,
    WorkspacePinned,
    GroupCollapsed,
    PanelTitle,
    PanelPinned,
    PanelUnread,
}

impl WriterKind {
    fn helper(self) -> &'static str {
        match self {
            Self::WorkspaceDescription => "set_workspace_description_for_control",
            Self::WorkspaceResetColor => "reset_workspace_color_for_control",
            Self::WorkspaceUnread => "set_workspace_unread_for_control",
            Self::WorkspacePinned => "set_workspace_pinned_for_control",
            Self::GroupCollapsed => "set_group_collapsed_for_control",
            Self::PanelTitle => "set_panel_title_for_control",
            Self::PanelPinned => "set_panel_pinned_for_control",
            Self::PanelUnread => "set_panel_unread_for_control",
        }
    }
}

const WRITERS: [WriterKind; 8] = [
    WriterKind::WorkspaceDescription,
    WriterKind::WorkspaceResetColor,
    WriterKind::WorkspaceUnread,
    WriterKind::WorkspacePinned,
    WriterKind::GroupCollapsed,
    WriterKind::PanelTitle,
    WriterKind::PanelPinned,
    WriterKind::PanelUnread,
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

fn add_workspace(snapshot: &mut AppSessionSnapshot, panel_id: &str) {
    apply_new_workspace(snapshot, panel_id, None, None, None, None);
}

fn add_group(snapshot: &mut AppSessionSnapshot) {
    snapshot.windows[0].tab_manager.workspace_groups =
        Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
            id: GROUP_ID.to_string(),
            name: "Group".to_string(),
            ..Default::default()
        }]);
}

fn workspace(snapshot: &AppSessionSnapshot, index: usize) -> &SessionWorkspaceSnapshot {
    &snapshot.windows[0].tab_manager.workspaces[index]
}

fn seed_for_changed(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = initial();
    match kind {
        WriterKind::WorkspaceResetColor => {
            snapshot.windows[0].tab_manager.workspaces[0].custom_color = Some("#ff00ff".into());
        }
        WriterKind::WorkspacePinned => add_workspace(&mut snapshot, "surface-2"),
        WriterKind::GroupCollapsed => add_group(&mut snapshot),
        _ => {}
    }
    snapshot
}

fn apply_changed(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::WorkspaceDescription => {
            apply_set_workspace_description(snapshot, 0, "  alpha\r\nbeta  ")
        }
        WriterKind::WorkspaceResetColor => apply_reset_workspace_color(snapshot, 0),
        WriterKind::WorkspaceUnread => {
            apply_set_workspace_unread_at(snapshot, 0, Some("surface-1"), true, WORKSPACE_UNREAD_AT)
        }
        WriterKind::WorkspacePinned => apply_set_workspace_pinned(snapshot, 1, true),
        WriterKind::GroupCollapsed => apply_set_group_collapsed(snapshot, GROUP_ID, true),
        WriterKind::PanelTitle => apply_set_panel_title(snapshot, "surface-1", "  Review  "),
        WriterKind::PanelPinned => apply_set_panel_pinned(snapshot, "surface-1", true),
        WriterKind::PanelUnread => {
            apply_set_panel_unread_at(snapshot, "surface-1", true, PANEL_UNREAD_AT)
        }
    }
}

fn seed_for_noop(kind: WriterKind) -> AppSessionSnapshot {
    let mut snapshot = seed_for_changed(kind);
    match kind {
        WriterKind::WorkspaceResetColor => {
            snapshot.windows[0].tab_manager.workspaces[0].custom_color = None;
        }
        _ => assert!(apply_changed(kind, &mut snapshot), "{}", kind.helper()),
    }
    snapshot
}

fn apply_noop(kind: WriterKind, snapshot: &mut AppSessionSnapshot) -> bool {
    match kind {
        WriterKind::WorkspaceDescription => {
            apply_set_workspace_description(snapshot, 0, "  alpha\nbeta  ")
        }
        WriterKind::WorkspaceResetColor => apply_reset_workspace_color(snapshot, 0),
        WriterKind::WorkspaceUnread => {
            apply_set_workspace_unread_at(snapshot, 0, Some("surface-1"), true, 99)
        }
        WriterKind::WorkspacePinned => apply_set_workspace_pinned(snapshot, 0, true),
        WriterKind::GroupCollapsed => apply_set_group_collapsed(snapshot, GROUP_ID, true),
        WriterKind::PanelTitle => apply_set_panel_title(snapshot, "surface-1", "Review"),
        WriterKind::PanelPinned => apply_set_panel_pinned(snapshot, "surface-1", true),
        WriterKind::PanelUnread => apply_set_panel_unread_at(snapshot, "surface-1", true, 99),
    }
}

fn assert_changed_semantics(kind: WriterKind, snapshot: &AppSessionSnapshot) {
    match kind {
        WriterKind::WorkspaceDescription => assert_eq!(
            workspace(snapshot, 0).custom_description.as_deref(),
            Some("  alpha\nbeta  ")
        ),
        WriterKind::WorkspaceResetColor => {
            assert_eq!(workspace(snapshot, 0).custom_color, None);
        }
        WriterKind::WorkspaceUnread => {
            let unread = &workspace(snapshot, 0).panel_unreads.as_ref().unwrap()[0];
            assert_eq!(
                (unread.panel_id.as_str(), unread.is_unread, unread.unread_at),
                ("surface-1", true, Some(WORKSPACE_UNREAD_AT))
            );
        }
        WriterKind::WorkspacePinned => {
            let tabs = &snapshot.windows[0].tab_manager;
            assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
            assert_eq!(tabs.selected_workspace_index, Some(0));
            assert_eq!(
                panel_ids_in_layout(tabs.workspaces[0].layout.as_ref().unwrap()),
                ["surface-2"]
            );
        }
        WriterKind::GroupCollapsed => {
            assert!(
                snapshot.windows[0]
                    .tab_manager
                    .workspace_groups
                    .as_ref()
                    .unwrap()[0]
                    .is_collapsed
            );
        }
        WriterKind::PanelTitle => assert_eq!(
            workspace(snapshot, 0).panel_titles.as_ref().unwrap()[0]
                .custom_title
                .as_deref(),
            Some("Review")
        ),
        WriterKind::PanelPinned => {
            assert!(workspace(snapshot, 0).panel_pins.as_ref().unwrap()[0].is_pinned);
        }
        WriterKind::PanelUnread => {
            let unread = &workspace(snapshot, 0).panel_unreads.as_ref().unwrap()[0];
            assert_eq!(
                (unread.is_unread, unread.unread_at),
                (true, Some(PANEL_UNREAD_AT))
            );
        }
    }
}

fn panel_ids_in_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<&str> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.panel_ids.iter().map(String::as_str).collect()
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = panel_ids_in_layout(&split.first);
            ids.extend(panel_ids_in_layout(&split.second));
            ids
        }
    }
}

#[test]
fn all_eight_changed_writers_publish_exact_normalized_metadata() {
    for kind in WRITERS {
        let before = seed_for_changed(kind);
        let mut expected = before.clone();
        assert!(apply_changed(kind, &mut expected), "{}", kind.helper());
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(kind, candidate)
        })
        .unwrap_or_else(|error| panic!("{}: {error}", kind.helper()));
        assert_eq!(committed, expected, "{}", kind.helper());
        assert_eq!(*authority.lock().unwrap(), expected, "{}", kind.helper());
        assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
        assert_eq!(publication.baseline, expected);
        assert_eq!(publication.events, [expected]);
        assert_changed_semantics(kind, &committed);
    }
}

#[test]
fn all_eight_same_or_missing_noops_preserve_timestamps_and_publish_nothing() {
    for kind in WRITERS {
        let before = seed_for_noop(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_noop(kind, candidate)
        })
        .unwrap_or_else(|error| panic!("{}: {error}", kind.helper()));
        assert_eq!(returned, before, "{}", kind.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.helper());
        assert!(publication.calls.is_empty(), "{}", kind.helper());
        assert_eq!(publication.baseline, before, "{}", kind.helper());
        assert!(publication.events.is_empty(), "{}", kind.helper());
    }
}

#[test]
fn all_eight_persist_failures_leak_no_candidate_authority_baseline_or_event() {
    for kind in WRITERS {
        let before = seed_for_changed(kind);
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        publication.persist_error = Some("injected metadata persistence failure".into());
        let error = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_changed(kind, candidate)
        })
        .unwrap_err();
        assert_eq!(error, "injected metadata persistence failure");
        assert_eq!(publication.calls, ["persist"], "{}", kind.helper());
        assert_eq!(*authority.lock().unwrap(), before, "{}", kind.helper());
        assert_eq!(publication.baseline, before, "{}", kind.helper());
        assert!(publication.events.is_empty(), "{}", kind.helper());
    }
}

#[test]
fn workspace_pin_reorders_ungrouped_selection_and_preserves_group_membership() {
    let mut snapshot = initial();
    add_workspace(&mut snapshot, "surface-2");
    add_workspace(&mut snapshot, "surface-3");
    add_workspace(&mut snapshot, "surface-4");
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(1);
    snapshot.windows[0].tab_manager.workspaces[0].group_id = Some(GROUP_ID.into());
    snapshot.windows[0].tab_manager.workspaces[2].group_id = Some(GROUP_ID.into());
    add_group(&mut snapshot);

    assert!(apply_set_workspace_pinned(&mut snapshot, 2, true));
    let tabs = &snapshot.windows[0].tab_manager;
    let order = tabs
        .workspaces
        .iter()
        .map(|workspace| panel_ids_in_layout(workspace.layout.as_ref().unwrap())[0])
        .collect::<Vec<_>>();
    assert_eq!(order, ["surface-1", "surface-3", "surface-2", "surface-4"]);
    assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(GROUP_ID));
    assert_eq!(tabs.workspaces[1].is_pinned, Some(true));
    assert_eq!(tabs.selected_workspace_index, Some(2));
}
