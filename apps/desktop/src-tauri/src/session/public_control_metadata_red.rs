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

    fn public(self) -> &'static str {
        match self {
            Self::WorkspaceDescription => "session_set_workspace_description",
            Self::WorkspaceResetColor => "session_reset_workspace_color",
            Self::WorkspaceUnread => "session_set_workspace_unread",
            Self::WorkspacePinned => "session_set_workspace_pinned",
            Self::GroupCollapsed => "session_set_group_collapsed",
            Self::PanelTitle => "session_set_panel_title",
            Self::PanelPinned => "session_set_panel_pinned",
            Self::PanelUnread => "session_set_panel_unread",
        }
    }

    fn route(self) -> (&'static str, &'static str) {
        match self {
            Self::WorkspaceDescription => ("workspace_set_description", "workspace_current("),
            Self::WorkspaceResetColor => ("workspace_reset_color", "workspace_current("),
            Self::WorkspaceUnread => ("workspace_set_unread", "workspace_current("),
            Self::WorkspacePinned => ("workspace_set_pinned", "workspace_current("),
            Self::GroupCollapsed => ("workspace_group_set_collapsed", "workspace_current("),
            Self::PanelTitle => ("surface_set_title", "surface_list_from_params("),
            Self::PanelPinned => ("surface_set_pinned", "surface_list_from_params("),
            Self::PanelUnread => ("surface_set_unread", "surface_list_from_params("),
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

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0usize;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

#[test]
fn exact_helpers_public_commands_sockets_and_web_callers_use_fallible_pairs() {
    let session = include_str!("../session.rs");
    let socket = include_str!("../control_socket.rs");
    let web = include_str!("../../../web/src/hooks/useSession.ts");
    for kind in WRITERS {
        let public = kind.public();
        let (route, success) = kind.route();
        let helper = function_source(session, &format!("pub(crate) fn {}(", kind.helper()));
        assert!(
            helper.contains("Result<AppSessionSnapshot, String>"),
            "{}",
            kind.helper()
        );
        assert!(
            helper.contains("state.transact_snapshot_if_changed(app,"),
            "{}",
            kind.helper()
        );
        assert!(!helper.contains("snapshot.lock()") && !helper.contains("notify_session_changed("));
        if matches!(kind, WriterKind::WorkspaceUnread | WriterKind::PanelUnread) {
            assert!(
                helper.contains("current_unix_timestamp_seconds()"),
                "{}",
                kind.helper()
            );
            assert!(helper.contains("_unread_at("), "{}", kind.helper());
        }

        let command = function_source(session, &format!("pub fn {public}("));
        assert!(
            command.contains("Result<AppSessionSnapshot, String>"),
            "{public}"
        );
        assert_eq!(command.matches(kind.helper()).count(), 1, "{public}");
        assert!(command.contains("Ok(snapshot)"), "{public}");
        assert!(
            !command.contains("apply_")
                && !command.contains("transact_")
                && !command.contains("snapshot.lock()")
                && !command.contains("notify_session_changed("),
            "{public}"
        );

        let route = function_source(socket, &format!("fn {route}("));
        assert_eq!(route.matches(kind.helper()).count(), 1, "{route}");
        assert!(route.contains(success), "{route}");
        assert!(route.contains("Err(message)"), "{route}");
        assert!(route.contains("\"internal\""), "{route}");

        assert!(
            web.contains(&format!(".invoke<AppSessionSnapshot>(\"{public}\"")),
            "{public}"
        );
        assert!(
            web.contains(&format!(
                ".catch((error) => console.error(\"{public} failed\", error))"
            )),
            "{public}"
        );
    }
}
